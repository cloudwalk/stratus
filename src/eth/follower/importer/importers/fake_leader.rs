use std::collections::BTreeSet;
use std::fmt::Write;
use std::sync::Arc;

use anyhow::bail;
use async_trait::async_trait;

use crate::GlobalState;
use crate::alias::RevmBytecode;
use crate::eth::executor::Executor;
use crate::eth::follower::importer::fetchers::DataFetcher;
use crate::eth::follower::importer::fetchers::fake_leader::FakeLeaderFetcher;
use crate::eth::follower::importer::importers::ImportData;
use crate::eth::follower::importer::importers::ImporterWorker;
use crate::eth::miner::Miner;
use crate::eth::miner::miner::interval_miner::commit_retry;
use crate::eth::miner::miner::interval_miner::mine_local_retry;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::EvmExecutionMetrics;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Index;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionError;
use crate::eth::storage::StratusStorage;

pub struct FakeLeaderWorker {
    pub executor: Arc<Executor>,
    pub miner: Arc<Miner>,
    pub storage: Arc<StratusStorage>,
}

impl ImportData for <FakeLeaderWorker as ImporterWorker>::DataType {
    fn block_number(&self) -> crate::eth::primitives::BlockNumber {
        self.0.block_number()
    }
}

#[async_trait]
impl ImporterWorker for FakeLeaderWorker {
    type DataType = <FakeLeaderFetcher as DataFetcher>::PostProcessType;

    async fn import(&self, ((block, _), (expected_block, expected_changes)): Self::DataType) -> anyhow::Result<usize> {
        let block_tx_len = block.transactions.len();
        self.storage.set_pending_from_external(&block);
        for tx in block.0.transactions.into_transactions() {
            tracing::info!(?tx, "executing tx as fake miner");
            if let Err(e) = self.executor.execute_local_transaction(tx.try_into()?) {
                match e {
                    StratusError::Transaction(TransactionError::Nonce { transaction: _, account: _ }) => {
                        tracing::warn!(reason = ?e, "transaction failed, was this node restarted?");
                    }
                    _ => {
                        tracing::error!(reason = ?e, "transaction failed");
                        GlobalState::shutdown_from("Importer (FakeMiner)", "Transaction Failed");
                        bail!(e);
                    }
                }
            }
        }
        let (mined_block, mut changes, miner_guard) = mine_local_retry(&self.miner);

        let completed_expected_changes = expected_changes.complete(self.storage.as_ref())?;
        changes.retain_modified();
        if changes != completed_expected_changes {
            let diff = diff_execution_changes(&changes, &completed_expected_changes);
            tracing::error!(diff = %diff, "execution changes result mismatch between leader and fake leader");
            bail!("execution changes mismatch between leader and fake leader")
        }

        // `expected_block` is built from `BlockRocksdb` (replicated), which drops per-tx
        // `changes` and `metrics` (replaced with `Default::default()` on the way back). Build a
        // normalized copy of the locally-mined block so the comparison checks fields that actually
        // survive replication, leaving the original untouched for commit.
        let normalized_mined_block = normalize_for_replication_compare(&mined_block);
        if normalized_mined_block != expected_block {
            let diff = diff_blocks(&normalized_mined_block, &expected_block);
            tracing::error!(diff = %diff, "block mismatch between leader and fake leader");
            bail!("block mismatch between leader and fake leader")
        }

        commit_retry(&self.miner, mined_block, changes, miner_guard);
        Ok(block_tx_len)
    }
}

/// Builds a copy of `block` with fields that do not survive `Block -> BlockRocksdb -> Block`
/// replication (or are version-skewed vs. the leader) reset to their defaults, so it can be
/// compared against the leader's replicated block for equivalence. Does not mutate the input.
///
/// `header.gas_used` is zeroed because the leader in production (commit `7c7d831`) predates
/// PR #2549, which started summing per-tx gas into the header. The fake leader computes the real
/// value, so comparing it directly trips on the version skew rather than a real divergence.
fn normalize_for_replication_compare(block: &Block) -> Block {
    let mut normalized = block.clone();
    normalized.header.gas_used = Gas::ZERO;
    for tx in &mut normalized.transactions {
        tx.execution.result.execution.changes = ExecutionChanges::default();
        tx.execution.result.metrics = EvmExecutionMetrics::default();
        // `first_log_index` does not survive `Block -> BlockRocksdb -> Block`: the rocks type
        // reconstructs it from the first log's index, defaulting to `ZERO` for txs with no logs
        // (`transaction_mined.rs`), whereas the freshly mined block carries the running cumulative
        // value. Normalize logless txs to `ZERO` to match the round-tripped leader block.
        if tx.execution.result.execution.logs.is_empty() {
            tx.mined_data.first_log_index = Index::ZERO;
        }
    }
    normalized
}

/// Builds a compact, human-readable diff of two [`Block`]s. Unlike logging the full
/// `?normalized_mined_block` / `?expected_block` (which dumps every transaction, log and bytecode,
/// blowing past log size limits), this only emits leaves whose value actually differs.
///
/// Fields already neutralized by [`normalize_for_replication_compare`] (per-tx `changes`,
/// `metrics`, logless `first_log_index`, `header.gas_used`) compare equal and thus do not appear.
fn diff_blocks(fake: &Block, leader: &Block) -> String {
    let fake_v = serde_json::to_value(fake).unwrap_or(serde_json::Value::Null);
    let leader_v = serde_json::to_value(leader).unwrap_or(serde_json::Value::Null);
    let mut out = String::new();
    diff_json("", &fake_v, &leader_v, &mut out);
    if out.is_empty() {
        out.push_str("<no field-level diff; Block PartialEq disagrees but no leaf differs>");
    }
    out
}

/// Recursively diffs two JSON values, appending one line per differing leaf in the form
/// `path: fake=<..> leader=<..>` (or `only in fake`/`only in leader`/`len mismatch`).
fn diff_json(path: &str, fake: &serde_json::Value, leader: &serde_json::Value, out: &mut String) {
    use serde_json::Value;
    match (fake, leader) {
        (Value::Object(a), Value::Object(b)) =>
            for key in a.keys().chain(b.keys()) {
                let child = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                match (a.get(key), b.get(key)) {
                    (Some(av), Some(bv)) => diff_json(&child, av, bv, out),
                    (Some(av), None) => {
                        let _ = writeln!(out, "{child}: only in fake = {av}");
                    }
                    (None, Some(bv)) => {
                        let _ = writeln!(out, "{child}: only in leader = {bv}");
                    }
                    (None, None) => {}
                }
            },
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                let _ = writeln!(out, "{path}: len mismatch fake={} leader={}", a.len(), b.len());
            }
            for (i, (av, bv)) in a.iter().zip(b.iter()).enumerate() {
                diff_json(&format!("{path}[{i}]"), av, bv, out);
            }
        }
        (a, b) =>
            if a != b {
                let _ = writeln!(out, "{path}: fake={a} leader={b}");
            },
    }
}

/// Builds a compact, human-readable diff of two completed [`ExecutionChanges`].
///
/// Unlike logging the full `?changes` / `?completed_expected_changes` (which dumps every account's
/// full bytecode and jump-table, blowing past log size limits), this only emits entries whose
/// `value` or `changed` flag actually differ between the fake leader and the real leader.
fn diff_execution_changes(fake: &ExecutionChanges, leader: &ExecutionChanges) -> String {
    let mut out = String::new();

    // accounts: iterate the union of touched addresses in deterministic order
    let addresses: BTreeSet<Address> = fake.accounts.keys().chain(leader.accounts.keys()).copied().collect();
    for address in addresses {
        match (fake.accounts.get(&address), leader.accounts.get(&address)) {
            (None, Some(leader_changes)) => {
                let _ = writeln!(out, "{address}: only in leader | {}", summarize_account_changes(leader_changes));
            }
            (Some(fake_changes), None) => {
                let _ = writeln!(out, "{address}: only in fake | {}", summarize_account_changes(fake_changes));
            }
            (Some(fake_changes), Some(leader_changes)) => {
                if fake_changes.nonce != leader_changes.nonce {
                    let _ = writeln!(
                        out,
                        "{address}.nonce: fake={}(changed={}) leader={}(changed={})",
                        fake_changes.nonce.value(),
                        fake_changes.nonce.is_changed(),
                        leader_changes.nonce.value(),
                        leader_changes.nonce.is_changed()
                    );
                }
                if fake_changes.balance != leader_changes.balance {
                    let _ = writeln!(
                        out,
                        "{address}.balance: fake={}(changed={}) leader={}(changed={})",
                        fake_changes.balance.value(),
                        fake_changes.balance.is_changed(),
                        leader_changes.balance.value(),
                        leader_changes.balance.is_changed()
                    );
                }
                if fake_changes.bytecode != leader_changes.bytecode {
                    let _ = writeln!(
                        out,
                        "{address}.bytecode: fake={}(changed={}) leader={}(changed={})",
                        summarize_bytecode(fake_changes.bytecode.value()),
                        fake_changes.bytecode.is_changed(),
                        summarize_bytecode(leader_changes.bytecode.value()),
                        leader_changes.bytecode.is_changed()
                    );
                }
            }
            (None, None) => unreachable!("address comes from one of the maps"),
        }
    }

    // slots: iterate the union of touched (address, index) pairs in deterministic order
    let slots: BTreeSet<(Address, SlotIndex)> = fake.slots.keys().chain(leader.slots.keys()).copied().collect();
    for (address, index) in slots {
        match (fake.slots.get(&(address, index)), leader.slots.get(&(address, index))) {
            (None, Some(leader_value)) => {
                let _ = writeln!(out, "{address}[{index}]: only in leader = {leader_value}");
            }
            (Some(fake_value), None) => {
                let _ = writeln!(out, "{address}[{index}]: only in fake = {fake_value}");
            }
            (Some(fake_value), Some(leader_value)) =>
                if fake_value != leader_value {
                    let _ = writeln!(out, "{address}[{index}]: fake={fake_value} leader={leader_value}");
                },
            (None, None) => unreachable!("slot comes from one of the maps"),
        }
    }

    if out.is_empty() {
        out.push_str("<no field-level diff; ExecutionChanges PartialEq disagrees but no account/slot field differs>");
    }
    out
}

/// One-line summary of an [`ExecutionAccountChanges`], compact enough for diff logging.
fn summarize_account_changes(changes: &ExecutionAccountChanges) -> String {
    format!(
        "nonce={}(changed={}) balance={}(changed={}) bytecode={}(changed={})",
        changes.nonce.value(),
        changes.nonce.is_changed(),
        changes.balance.value(),
        changes.balance.is_changed(),
        summarize_bytecode(changes.bytecode.value()),
        changes.bytecode.is_changed()
    )
}

/// Compact representation of a bytecode value: avoids dumping the full hex payload and jump-table.
fn summarize_bytecode(bytecode: &Option<RevmBytecode>) -> String {
    match bytecode {
        Some(code) => format!("Some(len={})", code.len()),
        None => "None".to_string(),
    }
}
