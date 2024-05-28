use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use daggy::stable_dag::StableDag;
use daggy::Walker;
use ethers_core::types::Transaction;
use futures::future::join_all;
use itertools::Itertools;
use petgraph::graph::NodeIndex;
use petgraph::visit::IntoNodeIdentifiers;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use super::primitives::Address;
use super::primitives::Block;
use super::primitives::BlockNumber;
use super::primitives::Index;
use super::primitives::SlotIndex;
use super::primitives::TransactionMined;
use crate::config::ExternalRelayerClientConfig;
use crate::config::ExternalRelayerServerConfig;
use crate::eth::evm::EvmExecutionResult;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::StratusStorage;
use crate::ext::ResultExt;
use crate::infra::blockchain_client::pending_transaction::PendingTransaction;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::BlockchainClient;
use crate::log_and_err;

pub struct TransactionRelayer {
    storage: Arc<StratusStorage>,

    /// RPC client that will submit transactions.
    chain: BlockchainClient,
}

impl TransactionRelayer {
    /// Creates a new [`TransactionRelayer`].
    pub fn new(storage: Arc<StratusStorage>, chain: BlockchainClient) -> Self {
        tracing::info!(?chain, "creating transaction relayer");
        Self { storage, chain }
    }

    /// Forwards the transaction to the external blockchain if the execution was successful on our side.
    #[tracing::instrument(skip_all)]
    pub async fn forward(&self, tx_input: TransactionInput, evm_result: EvmExecutionResult) -> anyhow::Result<()> {
        tracing::debug!(hash = %tx_input.hash, "forwarding transaction");

        // handle local failure
        if evm_result.is_failure() {
            tracing::debug!("transaction failed in local execution");
            let tx_execution = TransactionExecution::new_local(tx_input, evm_result);
            self.storage.save_execution(tx_execution).await?;
            return Ok(());
        }

        // handle local success
        let pending_tx = self
            .chain
            .send_raw_transaction(tx_input.hash, Transaction::from(tx_input.clone()).rlp())
            .await?;

        let Some(receipt) = pending_tx.await? else {
            return Err(anyhow!("transaction did not produce a receipt"));
        };

        let status = match receipt.status {
            Some(status) => status.as_u32(),
            None => return Err(anyhow!("receipt did not report the transaction status")),
        };

        if status == 0 {
            tracing::warn!(?receipt.transaction_hash, "transaction result mismatch between stratus and external rpc. saving to json");
            let mut file = File::create(format!("data/mismatched_transactions/{}.json", receipt.transaction_hash)).await?;
            let json = serde_json::json!(
                {
                    "transaction_input": tx_input,
                    "stratus_execution": evm_result,
                    "substrate_receipt": receipt
                }
            );
            file.write_all(json.to_string().as_bytes()).await?;
            return Err(anyhow!("transaction succeeded in stratus but failed in substrate"));
        }

        Ok(())
    }
}

pub struct ExternalRelayer {
    pool: PgPool,

    /// RPC client that will submit transactions.
    substrate_chain: BlockchainClient,
}

impl ExternalRelayer {
    /// Creates a new [`ExternalRelayer`].
    pub async fn new(config: ExternalRelayerServerConfig) -> anyhow::Result<Self> {
        tracing::info!(?config, "creating external relayer");
        let pool = PgPoolOptions::new()
            .min_connections(config.connections)
            .max_connections(config.connections)
            .acquire_timeout(config.acquire_timeout)
            .connect(&config.url)
            .await
            .expect("should not fail to create pgpool");

        Ok(Self {
            substrate_chain: BlockchainClient::new_http(&config.forward_to, config.rpc_timeout).await?,
            pool,
        })
    }

    /// Polls the next block to be relayed and relays it to Substrate.
    #[tracing::instrument(skip_all)]
    pub async fn relay_next_block(&self) -> anyhow::Result<Option<BlockNumber>> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let Some(row) = sqlx::query!(
            r#"UPDATE relayer_blocks
                SET relayed = true
                WHERE number = (
                    SELECT MIN(number)
                    FROM relayer_blocks
                    WHERE relayed = false
                )
                RETURNING payload"#
        )
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let block: Block = row.payload.try_into()?;
        let block_number = block.header.number;

        tracing::debug!(?block_number, "relaying block");

        // TODO: Replace failed transactions with transactions that will for sure fail in substrate (need access to primary keys)
        let dag = Self::compute_tx_dag(block.transactions);

        self.relay_dag(dag).await?; // do something with result?

        #[cfg(feature = "metrics")]
        metrics::inc_relay_next_block(start.elapsed());

        Ok(Some(block_number))
    }

    async fn compare_receipt(&self, stratus_receipt: ExternalReceipt, substrate_receipt: PendingTransaction<'_>) -> anyhow::Result<()> {
        match substrate_receipt.await {
            Ok(Some(substrate_receipt)) =>
                if let Err(compare_error) = substrate_receipt.compare(&stratus_receipt) {
                    let err_string = compare_error.to_string();
                    let error = log_and_err!("transaction mismatch!").context(err_string.clone());
                    self.save_mismatch(stratus_receipt, Some(substrate_receipt), &err_string).await;
                    error
                } else {
                    Ok(())
                },
            Ok(None) => {
                self.save_mismatch(stratus_receipt, None, "no receipt returned by substrate").await;
                Err(anyhow!("no receipt returned by substrate"))
            }
            Err(error) => {
                let error = error.context("failed to fetch substrate receipt");
                let err_str = error.to_string();
                self.save_mismatch(stratus_receipt, None, &err_str.to_string()).await;
                Err(error)
            }
        }
    }

    async fn save_mismatch(&self, stratus_receipt: ExternalReceipt, substrate_receipt: Option<ExternalReceipt>, err_string: &str) {
        let hash = stratus_receipt.hash().to_string();
        let stratus_json = serde_json::to_value(stratus_receipt).expect_infallible();
        let substrate_json = serde_json::to_value(substrate_receipt).expect_infallible();
        let res = sqlx::query!(
            "INSERT INTO mismatches (hash, stratus_receipt, substrate_receipt, error) VALUES ($1, $2, $3, $4)",
            &hash,
            &stratus_json,
            &substrate_json,
            err_string
        )
        .execute(&self.pool)
        .await; // should fallback is case of error?
        if let Err(err) = res {
            let mut file = File::create(format!("data/mismatched_transactions/{}.json", hash))
                .await
                .expect("opening the file should not fail");
            let json = serde_json::json!(
                {
                    "stratus_receipt": stratus_json,
                    "substrate_receipt": substrate_json,
                }
            );
            file.write_all(json.to_string().as_bytes())
                .await
                .expect("writing the mismatch to a file should not fail");
            tracing::error!(?err, "failed to save mismatch, saving to file");
        }
    }

    /// Uses the transactions and produces a Dependency DAG (Directed Acyclical Graph).
    /// Each vertex of the graph is a transaction, and two vertices are connected iff they conflict
    /// on either a slot or balance.
    /// The direction of an edge connecting the transactions A and B is always from
    /// `min(A.transaction_index, B.transaction_index)` to `max(A.transaction_index, B.transaction_index)`.
    /// Possible issues: this accounts for writes but not for reads, a transaction that reads a certain
    ///     slot but does not modify it would possibly be impacted by a transaction that does, meaning they
    ///     have a dependency that is not addressed here. Also there is a dependency between contract deployments
    ///     and contract calls that is not taken into consideration yet.
    /// If this algorithm is correct we could do away with StableDag and use StableGraph instead, for better performance
    #[tracing::instrument(skip_all)]
    fn compute_tx_dag(block_transactions: Vec<TransactionMined>) -> StableDag<TransactionMined, i32> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let mut slot_conflicts: HashMap<Index, HashSet<(Address, SlotIndex)>> = HashMap::new();
        let mut balance_conflicts: HashMap<Index, HashSet<Address>> = HashMap::new();
        let mut node_indexes: HashMap<Index, NodeIndex> = HashMap::new();
        let mut dag = StableDag::new();

        for tx in block_transactions.into_iter().sorted_by_key(|tx| tx.transaction_index) {
            let tx_idx = tx.transaction_index;
            for (address, change) in &tx.execution.changes {
                for (idx, slot_change) in &change.slots {
                    if slot_change.is_modified() {
                        slot_conflicts.entry(tx_idx).or_default().insert((*address, *idx));
                    }
                }

                if change.balance.is_modified() {
                    balance_conflicts.entry(tx_idx).or_default().insert(*address);
                }
            }
            let node_idx = dag.add_node(tx);
            node_indexes.insert(tx_idx, node_idx);
        }

        for (i, (tx1, set1)) in slot_conflicts.iter().sorted_by_key(|(idx, _)| **idx).enumerate() {
            for (tx2, set2) in slot_conflicts.iter().sorted_by_key(|(idx, _)| **idx).skip(i + 1) {
                if !set1.is_disjoint(set2) {
                    dag.add_edge(*node_indexes.get(tx1).unwrap(), *node_indexes.get(tx2).unwrap(), 1).unwrap();
                    // todo: unwrap -> expect
                }
            }
        }

        for (i, (tx1, set1)) in balance_conflicts.iter().sorted_by_key(|(idx, _)| **idx).enumerate() {
            for (tx2, set2) in balance_conflicts.iter().sorted_by_key(|(idx, _)| **idx).skip(i + 1) {
                if !set1.is_disjoint(set2) {
                    dag.add_edge(*node_indexes.get(tx1).unwrap(), *node_indexes.get(tx2).unwrap(), 1).unwrap();
                    // todo: unwrap -> expect
                }
            }
        }

        #[cfg(feature = "metrics")]
        metrics::inc_compute_tx_dag(start.elapsed());

        dag
    }

    /// Relays a transaction to Substrate and waits until the transaction is in the mempool by
    /// calling eth_getTransactionByHash.
    #[tracing::instrument(skip_all)]
    pub async fn relay_and_check_mempool(&self, tx_mined: TransactionMined) -> anyhow::Result<(PendingTransaction, ExternalReceipt)> {
        tracing::debug!(?tx_mined.input.hash, "relaying transaction");
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let ethers_tx = Transaction::from(tx_mined.input.clone());
        let tx = self.substrate_chain.send_raw_transaction(tx_mined.input.hash, ethers_tx.rlp()).await?;

        tracing::debug!(?tx_mined.input.hash, "polling eth_getTransactionByHash");
        let mut tries = 0;
        while self.substrate_chain.fetch_transaction(tx_mined.input.hash).await?.is_none() {
            if tries > 50 {
                return Err(anyhow!("transaction was not found in mempool after 500ms"));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
            tries += 1;
        }

        #[cfg(feature = "metrics")]
        metrics::inc_relay_and_check_mempool(start.elapsed());
        Ok((tx, ExternalReceipt(tx_mined.into())))
    }

    pub async fn relay(&self, tx_mined: TransactionMined) -> anyhow::Result<(PendingTransaction, ExternalReceipt)> {
        match self.relay_and_check_mempool(tx_mined.clone()).await {
            Err(err) => {
                let err = err.context("relay and check mempool failed");
                let err_string = err.to_string();
                self.save_mismatch(ExternalReceipt(tx_mined.into()), None, &err_string).await;
                Err(err)
            }
            ok => ok,
        }
    }

    /// Takes the roots (vertices with no parents) from the DAG, removing them from the graph,
    /// and by extension creating new roots for a future call. Returns `None` if the graph
    /// is empty.
    #[tracing::instrument(skip_all)]
    fn take_roots(dag: &mut StableDag<TransactionMined, i32>) -> Option<Vec<TransactionMined>> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let mut root_indexes = vec![];
        for index in dag.node_identifiers() {
            if dag.parents(index).walk_next(dag).is_none() {
                root_indexes.push(index);
            }
        }

        let mut roots = vec![];
        while let Some(root) = root_indexes.pop() {
            roots.push(dag.remove_node(root).unwrap()); // todo: unwrap -> expect
        }

        #[cfg(feature = "metrics")]
        metrics::inc_take_roots(start.elapsed());

        if roots.is_empty() {
            None
        } else {
            Some(roots)
        }
    }

    #[tracing::instrument(skip_all)]
    async fn relay_dag(&self, mut dag: StableDag<TransactionMined, i32>) -> anyhow::Result<()> {
        tracing::debug!("relaying transactions");
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let mut results = vec![];
        while let Some(roots) = Self::take_roots(&mut dag) {
            let futures = roots.into_iter().map(|root_tx| self.relay(root_tx));
            for result in join_all(futures).await {
                match result {
                    Ok(res) => results.push(res),
                    Err(err) => tracing::error!(?err),
                }
            }
        }

        let futures = results
            .into_iter()
            .map(|(substrate_pending_tx, stratus_receipt)| self.compare_receipt(stratus_receipt, substrate_pending_tx));

        join_all(futures).await; // xxx: do something with the result

        #[cfg(feature = "metrics")]
        metrics::inc_relay_dag(start.elapsed());

        Ok(())
    }
}

pub struct ExternalRelayerClient {
    pool: PgPool,
}

impl ExternalRelayerClient {
    /// Creates a new [`ExternalRelayerClient`].
    pub async fn new(config: ExternalRelayerClientConfig) -> Self {
        tracing::info!(?config, "creating external relayer client");
        let storage = PgPoolOptions::new()
            .min_connections(config.connections)
            .max_connections(config.connections)
            .acquire_timeout(config.acquire_timeout)
            .connect(&config.url)
            .await
            .expect("should not fail to create pgpool");

        Self { pool: storage }
    }

    pub async fn send_to_relayer(&self, block: Block) -> anyhow::Result<()> {
        tracing::debug!(?block.header.number, "sending block to relayer");
        let block_number = block.header.number;
        sqlx::query!("INSERT INTO relayer_blocks VALUES ($1, $2)", block_number as _, serde_json::to_value(block)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::ExternalRelayer;
    use crate::eth::primitives::Address;
    use crate::eth::primitives::Bytes;
    use crate::eth::primitives::CodeHash;
    use crate::eth::primitives::EvmExecution;
    use crate::eth::primitives::ExecutionAccountChanges;
    use crate::eth::primitives::ExecutionResult;
    use crate::eth::primitives::ExecutionValueChange;
    use crate::eth::primitives::Gas;
    use crate::eth::primitives::Hash;
    use crate::eth::primitives::Slot;
    use crate::eth::primitives::SlotIndex;
    use crate::eth::primitives::TransactionInput;
    use crate::eth::primitives::TransactionMined;
    use crate::eth::primitives::UnixTime;
    const ADDRESS: Address = Address::ZERO;

    fn create_tx(changed_slots_inidices: HashSet<SlotIndex>, tx_idx: u64) -> TransactionMined {
        let execution_changes = ExecutionAccountChanges {
            new_account: false,
            address: ADDRESS,
            nonce: ExecutionValueChange::default(),
            balance: ExecutionValueChange::default(),
            bytecode: ExecutionValueChange::default(),
            code_hash: CodeHash::default(),
            static_slot_indexes: ExecutionValueChange::default(),
            mapping_slot_indexes: ExecutionValueChange::default(),
            slots: changed_slots_inidices
                .into_iter()
                .map(|index| (index, ExecutionValueChange::from_modified(Slot { index, value: 0.into() })))
                .collect(),
        };
        let execution = EvmExecution {
            block_timestamp: UnixTime::default(),
            receipt_applied: false,
            result: ExecutionResult::Success,
            output: Bytes::default(),
            logs: vec![],
            gas: Gas::default(),
            changes: [(ADDRESS, execution_changes)].into_iter().collect(),
            deployed_contract_address: None,
        };

        TransactionMined {
            input: TransactionInput::default(),
            execution,
            logs: vec![],
            transaction_index: tx_idx.into(),
            block_number: 0.into(),
            block_hash: Hash::default(),
        }
    }

    #[test]
    fn test_compute_tx_dag_and_take_roots() {
        let expected1 = vec![vec![0, 1], vec![2], vec![3], vec![4, 5], vec![6]];
        let transactions1 = vec![
            vec![1],       // (0): dag root
            vec![2],       // (1): dag root
            vec![1, 2, 3], // (2): depends on (0) and (1)
            vec![3, 4, 5], // (3): depends on (2)
            vec![4, 7],    // (4): depends on (3)
            vec![3, 8],    // (5): depends on (3)
            vec![8, 7],    // (6): depends on (4) and (5)
        ];

        let expected2 = vec![vec![0], vec![1, 2], vec![3, 4], vec![5, 6, 7, 8], vec![9]];
        let transactions2 = vec![
            vec![1, 2],           // (0): dag root
            vec![1, 3],           // (1): depends on (0)
            vec![2, 7],           // (2): depends on (0)
            vec![3, 4, 5],        // (3): depends on (1)
            vec![7, 8, 9],        // (4): depends on (2)
            vec![4, 10],          // (5): depends on (3)
            vec![5, 11],          // (6): depends on (3)
            vec![8, 12],          // (7): depends on (4)
            vec![9, 13],          // (8): depends on (4)
            vec![10, 11, 12, 13], // (9): depends on (5), (6), (7) and (8)
        ];

        let expected3 = vec![vec![0, 2, 3], vec![1], vec![4], vec![5, 7], vec![6, 10], vec![8, 11], vec![9]];
        let transactions3 = vec![
            vec![1],                  // (0): dag root
            vec![1, 2, 3],            // (1): depends on (0)
            vec![13],                 // (2): dag root
            vec![14, 15],             // (3): dag root
            vec![2, 4, 5, 6, 13, 14], // (4): depends on (2) and (3)
            vec![4, 12, 15, 16],      // (5): depends on (3) and (4)
            vec![5, 9, 16],           // (6): depends on (4) and (5)
            vec![3, 6, 7, 10],        // (7): depends on (1) and (4),
            vec![9, 10, 11, 12],      // (8): depends on (5), (6) and (7)
            vec![11],                 // (9): depends on (8)
            vec![7, 8],               // (10): depends on (7)
            vec![8],                  // (11): depends on (10)
        ];

        let tests = [transactions1, transactions2, transactions3];
        let expected_results = [expected1, expected2, expected3];

        for (test, expected) in tests.into_iter().zip(expected_results) {
            let transactions = test
                .into_iter()
                .map(|indexes| indexes.into_iter().map(SlotIndex::from))
                .enumerate()
                .map(|(i, indexes)| create_tx(indexes.collect(), i as u64))
                .collect();

            let mut dag = ExternalRelayer::compute_tx_dag(transactions);
            let mut i = 0;
            while let Some(roots) = ExternalRelayer::take_roots(&mut dag) {
                assert_eq!(roots.len(), expected[i].len());
                assert!(roots.iter().all(|tx| expected[i].contains(&tx.transaction_index.inner_value())));
                i += 1;
            }
        }
    }
}
