//! Concrete Implementation of the `Evm` Trait Using `revm`
//!
//! `Revm` is a concrete implementation of the Evm trait, utilizing the `revm` library.
//! It translates the abstract requirements of Ethereum transaction execution into actionable logic,
//! interacting with the project's storage backend to manage state. `Revm` embodies the practical application
//! of the `Evm` trait, serving as a bridge between Ethereum's abstract operations and Stratus's storage mechanisms.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use anyhow::anyhow;
use ethers_core::utils::keccak256;
use itertools::Itertools;
use revm::interpreter::analysis::to_analysed;
use revm::interpreter::InstructionResult;
use revm::primitives::AccountInfo as RevmAccountInfo;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Context;
use itertools::Itertools;
use revm::primitives::AccountInfo;
use revm::primitives::Address as RevmAddress;
use revm::primitives::Bytecode as RevmBytecode;
use revm::primitives::CreateScheme;
use revm::primitives::ExecutionResult as RevmExecutionResult;
use revm::primitives::ResultAndState as RevmResultAndState;
use revm::primitives::SpecId;
use revm::primitives::State as RevmState;
use revm::primitives::TransactTo;
use revm::primitives::B256;
use revm::primitives::KECCAK_EMPTY;
use revm::primitives::U256;
use revm::Database;
use revm::EVM;
use tokio::runtime::Handle;

use crate::eth::evm::evm::EvmExecutionResult;
use crate::eth::evm::Evm;
use crate::eth::evm::EvmInput;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BytecodeHash;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExecutionMetrics;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::ExecutionValueChange;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Log;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::storage::StratusStorage;
use crate::ext::not;
use crate::ext::OptionExt;
use crate::infra::metrics;

/// Implementation of EVM using [`revm`](https://crates.io/crates/revm).
pub struct Revm {
    evm: EVM<RevmDatabaseSession>,
    storage: Arc<StratusStorage>,

    /// Analysed bytecodes.
    bytecodes: Arc<RwLock<HashMap<BytecodeHash, RevmBytecode>>>,
}

impl Revm {
    /// Creates a new instance of the Revm ready to be used.
    pub fn new(storage: Arc<StratusStorage>) -> Self {
        let mut evm = EVM::new();

        // evm general config
        evm.env.cfg.spec_id = SpecId::LONDON;
        evm.env.cfg.limit_contract_code_size = Some(usize::MAX);
        evm.env.block.coinbase = Address::COINBASE.into();

        // evm tx config
        evm.env.tx.gas_priority_fee = None;

        Self {
            evm,
            storage,
            bytecodes: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Evm for Revm {
    fn execute(&mut self, input: EvmInput) -> anyhow::Result<EvmExecutionResult> {
        let start = Instant::now();

        // configure session
        let evm = &mut self.evm;
        let session = RevmDatabaseSession::new(Arc::clone(&self.storage), input.clone(), Arc::clone(&self.bytecodes));
        evm.database(session);

        // configure evm block
        evm.env.block.basefee = U256::ZERO;
        evm.env.block.timestamp = input.block_timestamp.into();
        evm.env.block.number = input.block_number.into();

        // configure tx params
        let tx = &mut evm.env.tx;
        tx.caller = input.from.into();
        tx.transact_to = match input.to {
            Some(contract) => TransactTo::Call(contract.into()),
            None => TransactTo::Create(CreateScheme::Create),
        };
        tx.gas_limit = input.gas_limit.into();
        tx.gas_price = input.gas_price.into();
        tx.nonce = input.nonce.map_into();
        tx.data = input.data.into();
        tx.value = input.value.into();

        // execute evm
        let evm_result = evm.transact();

        // parse result and track metrics
        let session = evm.take_db();
        let metrics = session.metrics;
        let point_in_time = session.input.point_in_time.clone();

        let execution = match evm_result {
                let session = evm.take_db();

                // cache new bytecodes generated during this transaction
                let has_new_bytecodes = session.has_new_bytecodes();
                let result = parse_revm_execution(result, session.input, session.storage_changes)?;
                if result.is_success() && has_new_bytecodes {
                    let mut bytecodes_write_lock = self.bytecodes.write().unwrap();
                    for (bytecode_hash, bytecode) in session.new_analysed_bytecodes {
                        bytecodes_write_lock.insert(bytecode_hash, bytecode);
                    }
                    drop(bytecodes_write_lock);
                }

                Ok(result)
            }
            Err(e) => {
                tracing::warn!(reason = ?e, "evm execution error");
                Err(e).context("Error executing EVM transaction.")
            }
        };

        metrics::inc_evm_execution(start.elapsed(), &point_in_time, execution.is_ok());
        metrics::inc_evm_execution_account_reads(metrics.account_reads);
        metrics::inc_evm_execution_slot_reads(metrics.slot_reads);

        execution.map(|x| (x, metrics))
    }
}

// -----------------------------------------------------------------------------
// Database
// -----------------------------------------------------------------------------

/// Contextual data that is read or set durint the execution of a transaction in the EVM.
struct RevmDatabaseSession {
    /// Service to communicate with the storage.
    storage: Arc<StratusStorage>,

    /// Input passed to EVM to execute the transaction.
    input: EvmInput,

    /// Changes made to the storage during the execution of the transaction.
    storage_changes: ExecutionChanges,

    /// All analysed bytecodes that are cached.
    all_analysed_bytecodes: Arc<RwLock<HashMap<BytecodeHash, RevmBytecode>>>,

    /// New analysed bytecodes crated during this transaction execution.
    new_analysed_bytecodes: Vec<(BytecodeHash, RevmBytecode)>,
  
    /// Metrics collected during EVM execution.
    metrics: ExecutionMetrics,
}

impl RevmDatabaseSession {
    /// Creates a new [`RevmDatabaseSession`].
    pub fn new(storage: Arc<StratusStorage>, input: EvmInput, all_bytecodes: Arc<RwLock<HashMap<BytecodeHash, RevmBytecode>>>) -> Self {
        Self {
            storage,
            input,
            storage_changes: Default::default()
            all_analysed_bytecodes: all_bytecodes,
            new_analysed_bytecodes: Vec::new(),
            metrics: Default::default(),
        }
    }

    /// Indicates if new bytecodes were analysed during the transaction execution associated with this session.
    pub fn has_new_bytecodes(&self) -> bool {
        not(self.new_analysed_bytecodes.is_empty())
    }
}

impl Database for RevmDatabaseSession {
    type Error = anyhow::Error;

    fn basic(&mut self, revm_address: RevmAddress) -> anyhow::Result<Option<AccountInfo>> {
        self.metrics.account_reads += 1;

        // retrieve account
        let address: Address = revm_address.into();
        let account = Handle::current().block_on(self.storage.read_account(&address, &self.input.point_in_time))?;

        // warn if the loaded account is the `to` account and it does not have a bytecode
        if let Some(ref to_address) = self.input.to {
            if &address == to_address && not(account.is_contract()) {
                tracing::warn!(%address, "evm to_account does not have bytecode");
            }
        }

        // track original value, except if ignored address
        if not(account.address.is_ignored()) {
            self.storage_changes
                .insert(account.address.clone(), ExecutionAccountChanges::from_existing_account(account.clone()));
        }

        // get bytecode from cache or analyse and cache it
        let mut bytecode: Option<RevmBytecode> = None;
        if let Some(bytes) = account.bytecode {
            // TODO: maybe we should use the code_hash attribute from account here?
            let bytecode_hash = keccak256(bytes.as_ref());

            let bytecodes_read_lock = self.all_analysed_bytecodes.read().unwrap();
            if let Some(cached_bytecode) = bytecodes_read_lock.get(&bytecode_hash) {
                bytecode = Some(cached_bytecode.clone());
            } else {
                let analysed_bytecode = to_analysed(bytes.into());
                self.new_analysed_bytecodes.push((bytecode_hash, analysed_bytecode.clone()));
                bytecode = Some(analysed_bytecode);
            }
        }

        Ok(Some(RevmAccountInfo {
            nonce: account.nonce.into(),
            balance: account.balance.into(),
            code_hash: KECCAK_EMPTY,
            code: bytecode,
        }))
    }

    fn code_by_hash(&mut self, _: B256) -> anyhow::Result<RevmBytecode> {
        todo!()
    }

    fn storage(&mut self, revm_address: RevmAddress, revm_index: U256) -> anyhow::Result<U256> {
        self.metrics.slot_reads += 1;

        // retrieve slot
        let address: Address = revm_address.into();
        let index: SlotIndex = revm_index.into();
        let slot = Handle::current().block_on(self.storage.read_slot(&address, &index, &self.input.point_in_time))?;

        // track original value, except if ignored address
        if not(address.is_ignored()) {
            match self.storage_changes.get_mut(&address) {
                Some(account) => {
                    account.slots.insert(index, ExecutionValueChange::from_original(slot.clone()));
                }
                None => {
                    tracing::error!(reason = "reading slot without account loaded", %address, %index);
                    return Err(anyhow!("Account '{}' was expected to be loaded by EVM, but it was not", address));
                }
            };
        }

        Ok(slot.value.into())
    }

    fn block_hash(&mut self, _: U256) -> anyhow::Result<B256> {
        todo!()
    }
}

// -----------------------------------------------------------------------------
// Conversion
// -----------------------------------------------------------------------------

fn parse_revm_execution(revm_result: RevmResultAndState, input: EvmInput, execution_changes: ExecutionChanges) -> Execution {
    let (result, output, logs, gas) = parse_revm_result(revm_result.result);
    let execution_changes = parse_revm_state(revm_result.state, execution_changes);

    tracing::info!(?result, %gas, output_len = %output.len(), %output, "evm executed");
    Execution {
        result,
        output,
        logs,
        gas,
        block_timestamp: input.block_timestamp,
        changes: execution_changes.into_values().collect(),
    }
}

fn parse_revm_result(result: RevmExecutionResult) -> (ExecutionResult, Bytes, Vec<Log>, Gas) {
    match result {
        RevmExecutionResult::Success { output, gas_used, logs, .. } => {
            let result = ExecutionResult::Success;
            let output = Bytes::from(output);
            let logs = logs.into_iter().map_into().collect();
            let gas = Gas::from(gas_used);
            (result, output, logs, gas)
        }
        RevmExecutionResult::Revert { output, gas_used } => {
            let result = ExecutionResult::Reverted;
            let output = Bytes::from(output);
            let gas = Gas::from(gas_used);
            (result, output, Vec::new(), gas)
        }
        RevmExecutionResult::Halt { reason, gas_used } => {
            let result = ExecutionResult::new_halted(format!("{:?}", reason));
            let output = Bytes::default();
            let gas = Gas::from(gas_used);
            (result, output, Vec::new(), gas)
        }
    }
}

fn parse_revm_state(revm_state: RevmState, mut execution_changes: ExecutionChanges) -> ExecutionChanges {
    for (revm_address, revm_account) in revm_state {
        let address: Address = revm_address.into();
        if address.is_ignored() {
            continue;
        }

        // apply changes according to account status
        tracing::debug!(
            %address,
            status = ?revm_account.status,
            balance = %revm_account.info.balance,
            nonce = %revm_account.info.nonce,
            slots = %revm_account.storage.len(),
            "evm account"
        );
        let (account_created, account_updated) = (revm_account.is_created(), revm_account.is_touched());

        // parse revm to internal representation
        let account: Account = (revm_address, revm_account.info).into();
        let account_modified_slots: Vec<Slot> = revm_account
            .storage
            .into_iter()
            .map(|(index, value)| Slot::new(index, value.present_value))
            .collect();

        // status: created
        if account_created {
            execution_changes.insert(
                account.address.clone(),
                ExecutionAccountChanges::from_new_account(account, account_modified_slots),
            );
        }
        // status: touched (updated)
        else if account_updated {
            let Some(existing_account) = execution_changes.get_mut(&address) else {
                tracing::error!(keys = ?execution_changes.keys(), %address, "account updated, but not loaded by evm");
                // TODO: panic! only when in dev-mode or try to refactor to avoid panic!
                panic!("Account '{}' was expected to be loaded by EVM, but it was not", address);
            };
            existing_account.apply_changes(account, account_modified_slots);
        }
    }
    execution_changes
}
