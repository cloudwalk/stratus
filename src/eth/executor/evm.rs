use std::cmp::min;
use std::collections::HashMap;
use std::sync::Arc;

use alloy_consensus::transaction::TransactionInfo;
use alloy_rpc_types_trace::geth::call::FlatCallFrame;
use alloy_rpc_types_trace::geth::mux::MuxFrame;
use alloy_rpc_types_trace::geth::CallFrame;
use alloy_rpc_types_trace::geth::FourByteFrame;
use alloy_rpc_types_trace::geth::GethDebugBuiltInTracerType;
use alloy_rpc_types_trace::geth::GethDebugTracerType;
use alloy_rpc_types_trace::geth::GethTrace;
use alloy_rpc_types_trace::geth::NoopFrame;
use anyhow::anyhow;
use itertools::Itertools;
use revm::db::CacheDB;
use revm::inspector_handle_register;
use revm::primitives::AccountInfo;
use revm::primitives::AnalysisKind;
use revm::primitives::BlockEnv;
use revm::primitives::EVMError;
use revm::primitives::EVMResult;
use revm::primitives::EvmState;
use revm::primitives::ExecutionResult as RevmExecutionResult;
use revm::primitives::InvalidTransaction;
use revm::primitives::ResultAndState as RevmResultAndState;
use revm::primitives::SpecId;
use revm::primitives::TransactTo;
use revm::primitives::TxEnv;
use revm::primitives::B256;
use revm::primitives::U256;
use revm::Database;
use revm::DatabaseRef;
use revm::Evm as RevmEvm;
use revm::GetInspector;
use revm::Handler;
use revm_inspectors::tracing::FourByteInspector;
use revm_inspectors::tracing::MuxInspector;
use revm_inspectors::tracing::TracingInspector;
use revm_inspectors::tracing::TracingInspectorConfig;

use super::evm_input::InspectorInput;
use crate::alias::RevmAddress;
use crate::alias::RevmBytecode;
use crate::eth::executor::EvmExecutionResult;
use crate::eth::executor::EvmInput;
use crate::eth::executor::ExecutorConfig;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::EvmExecutionMetrics;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::ExecutionValueChange;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Log;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionError;
use crate::eth::primitives::TransactionStage;
use crate::eth::primitives::UnexpectedError;
use crate::eth::storage::StratusStorage;
use crate::ext::not;
use crate::ext::OptionExt;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

/// Maximum gas limit allowed for a transaction. Prevents a transaction from consuming too many resources.
const GAS_MAX_LIMIT: u64 = 1_000_000_000;

/// Implementation of EVM using [`revm`](https://crates.io/crates/revm).
pub struct Evm {
    evm: RevmEvm<'static, (), RevmSession>,
}

impl Evm {
    /// Creates a new instance of the Evm.
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new(storage: Arc<StratusStorage>, config: ExecutorConfig) -> Self {
        tracing::info!(?config, "creating revm");

        // configure handler
        let mut handler = Handler::mainnet_with_spec(SpecId::LONDON);

        // handler custom validators
        let validate_tx_against_state = handler.validation.tx_against_state;
        handler.validation.tx_against_state = Arc::new(move |ctx| {
            let result = validate_tx_against_state(ctx);
            if result.is_err() {
                let _ = ctx.evm.inner.journaled_state.finalize(); // clear revm state on validation failure
            }
            result
        });

        // handler custom instructions
        let instructions = handler.take_instruction_table();
        handler.set_instruction_table(instructions);

        // configure revm
        let chain_id = config.executor_chain_id;
        let mut evm = RevmEvm::builder()
            .with_external_context(())
            .with_db(RevmSession::new(storage, config))
            .with_handler(handler)
            .build();

        // global general config
        let cfg_env = evm.cfg_mut();
        cfg_env.chain_id = chain_id;
        cfg_env.limit_contract_code_size = Some(usize::MAX);
        cfg_env.perf_analyse_created_bytecodes = AnalysisKind::Analyse;

        // global block config
        let block_env = evm.block_mut();
        block_env.coinbase = Address::COINBASE.into();

        // global tx config
        let tx_env = evm.tx_mut();
        tx_env.gas_priority_fee = None;

        Self { evm }
    }

    /// Execute a transaction that deploys a contract or call a contract function.
    pub fn execute(&mut self, input: EvmInput) -> Result<EvmExecutionResult, StratusError> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        // configure session
        let evm = &mut self.evm;
        evm.db_mut().reset(input.clone());

        evm.fill_env(input);

        let block_env_log = evm.block().clone();
        let tx_env_log = evm.tx().clone();

        // execute transaction
        tracing::info!(block_env = ?block_env_log, tx_env = ?tx_env_log, "executing transaction in revm");
        let evm_result = evm.transact();

        // extract results
        let session = evm.db_mut();
        let session_input = std::mem::take(&mut session.input);
        let session_storage_changes = std::mem::take(&mut session.storage_changes);
        let session_metrics = std::mem::take(&mut session.metrics);
        #[cfg(feature = "metrics")]
        let session_point_in_time = std::mem::take(&mut session.input.point_in_time);

        // parse result
        let execution = match evm_result {
            // executed
            Ok(result) => Ok(parse_revm_execution(result, session_input, session_storage_changes)?),

            // nonce errors
            Err(EVMError::Transaction(InvalidTransaction::NonceTooHigh { tx, state })) => Err(TransactionError::Nonce {
                transaction: tx.into(),
                account: state.into(),
            }
            .into()),
            Err(EVMError::Transaction(InvalidTransaction::NonceTooLow { tx, state })) => Err(TransactionError::Nonce {
                transaction: tx.into(),
                account: state.into(),
            }
            .into()),

            // storage error
            Err(EVMError::Database(e)) => {
                tracing::warn!(reason = ?e, "evm storage error");
                Err(e)
            }

            // unexpected errors
            Err(e) => {
                tracing::warn!(reason = ?e, "evm transaction error");
                Err(TransactionError::EvmFailed(e.to_string()).into())
            }
        };

        // track metrics
        #[cfg(feature = "metrics")]
        {
            metrics::inc_evm_execution(start.elapsed(), session_point_in_time, execution.is_ok());
            metrics::inc_evm_execution_account_reads(session_metrics.account_reads);
        }

        execution.map(|execution| EvmExecutionResult {
            execution,
            metrics: session_metrics,
        })
    }

    /// Execute a transaction using a tracer.
    pub fn inspect(&mut self, input: InspectorInput) -> Result<GethTrace, StratusError> {
        let InspectorInput {
            tx_hash,
            opts,
            trace_unsuccessful_only,
        } = input;
        let tracer_type = opts.tracer.ok_or_else(|| anyhow!("no tracer type provided"))?;

        if matches!(tracer_type, GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::NoopTracer)) {
            return Ok(NoopFrame::default().into());
        }

        let tx = self
            .evm
            .db()
            .storage
            .read_transaction(tx_hash)?
            .ok_or_else(|| anyhow!("transaction not found: {}", tx_hash))?;

        if trace_unsuccessful_only && matches!(tx.result(), ExecutionResult::Success) {
            return Ok(default_trace(tracer_type, tx));
        }

        let block = self.evm.db().storage.read_block(BlockFilter::Number(tx.block_number()))?.ok_or_else(|| {
            StratusError::Storage(StorageError::BlockNotFound {
                filter: BlockFilter::Number(tx.block_number()),
            })
        })?;

        let tx_info = TransactionInfo {
            block_hash: Some(block.hash().0 .0.into()),
            hash: Some(tx_hash.0 .0.into()),
            index: tx.index().map_into(),
            block_number: Some(block.number().as_u64()),
            base_fee: None,
        };
        let inspect_input: EvmInput = tx.try_into()?;
        self.evm.db_mut().reset(EvmInput {
            point_in_time: PointInTime::MinedPast(inspect_input.block_number.prev().unwrap_or_default()),
            ..Default::default()
        });
        let mut cache_db = CacheDB::new(self.evm.db());
        let handler = Handler::mainnet_with_spec(SpecId::LONDON);

        let mut evm = RevmEvm::builder()
            .with_external_context(())
            .with_db(&mut cache_db)
            .with_handler(handler)
            .build();

        evm.cfg_mut().chain_id = inspect_input.chain_id.unwrap_or_default().into();

        // Execute all transactions before target tx_hash
        for tx in block.transactions.into_iter().sorted_by_key(|item| item.transaction_index) {
            if tx.input.hash == tx_hash {
                break;
            }
            let tx_input = tx.into();

            // Configure EVM state
            evm.fill_env(tx_input);

            evm.transact_commit()?;
        }

        drop(evm);

        let trace_result: GethTrace = match tracer_type {
            GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::FourByteTracer) => {
                let mut inspector = FourByteInspector::default();
                RevmEvm::<(), _>::inspect(inspect_input, cache_db, &mut inspector)?;
                FourByteFrame::from(&inspector).into()
            }
            GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::CallTracer) => {
                let call_config = opts.tracer_config.into_call_config()?;
                let mut inspector = TracingInspector::new(TracingInspectorConfig::from_geth_call_config(&call_config));
                let res = RevmEvm::<(), _>::inspect(inspect_input, cache_db, &mut inspector)?;
                inspector.geth_builder().geth_call_traces(call_config, res.result.gas_used()).into()
            }
            GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::PreStateTracer) => {
                let prestate_config = opts.tracer_config.into_pre_state_config()?;
                let mut inspector = TracingInspector::new(TracingInspectorConfig::from_geth_prestate_config(&prestate_config));
                let res = RevmEvm::<(), _>::inspect(inspect_input, &mut cache_db, &mut inspector)?;
                inspector.geth_builder().geth_prestate_traces(&res, &prestate_config, &cache_db)?.into()
            }
            GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::NoopTracer) => NoopFrame::default().into(),
            GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::MuxTracer) => {
                let mux_config = opts.tracer_config.into_mux_config()?;
                let mut inspector = MuxInspector::try_from_config(mux_config).map_err(|e| anyhow!(e))?;
                let res = RevmEvm::<(), _>::inspect(inspect_input, &mut cache_db, &mut inspector)?;
                inspector.try_into_mux_frame(&res, &cache_db, tx_info)?.into()
            }
            GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::FlatCallTracer) => {
                let flat_call_config = opts.tracer_config.into_flat_call_config()?;
                let mut inspector = TracingInspector::new(TracingInspectorConfig::from_flat_call_config(&flat_call_config));
                let res = RevmEvm::<(), _>::inspect(inspect_input, &mut cache_db, &mut inspector)?;
                inspector
                    .with_transaction_gas_limit(res.result.gas_used())
                    .into_parity_builder()
                    .into_localized_transaction_traces(tx_info)
                    .into()
            }
            _ => return Err(anyhow!("tracer not implemented").into()),
        };

        Ok(trace_result)
    }
}

trait TxEnvExt {
    fn fill_env(&mut self, input: EvmInput);
}

trait EvmExt<DB: Database> {
    fn fill_env(&mut self, input: EvmInput);
    fn inspect(input: EvmInput, db: DB, inspector: impl GetInspector<DB>) -> EVMResult<DB::Error>;
}

impl<EXT, DB: Database> EvmExt<DB> for RevmEvm<'_, EXT, DB> {
    fn fill_env(&mut self, input: EvmInput) {
        self.block_mut().fill_env(&input);
        self.tx_mut().fill_env(input);
    }

    fn inspect(input: EvmInput, db: DB, inspector: impl GetInspector<DB>) -> EVMResult<DB::Error> {
        let handler = Handler::mainnet_with_spec(SpecId::LONDON);

        let mut evm = RevmEvm::builder()
            .with_external_context(inspector)
            .with_db(db)
            .with_handler(handler)
            .append_handler_register(inspector_handle_register)
            .build();

        evm.cfg_mut().chain_id = input.chain_id.unwrap_or_default().into();
        evm.fill_env(input);

        evm.transact()
    }
}

impl TxEnvExt for TxEnv {
    fn fill_env(&mut self, input: EvmInput) {
        self.caller = input.from.into();
        self.transact_to = match input.to {
            Some(contract) => TransactTo::Call(contract.into()),
            None => TransactTo::Create,
        };
        self.gas_limit = min(input.gas_limit.into(), GAS_MAX_LIMIT);
        self.gas_price = input.gas_price.into();
        self.chain_id = input.chain_id.map_into();
        self.nonce = input.nonce.map_into();
        self.data = input.data.into();
        self.value = input.value.into();
    }
}

trait BlockEnvExt {
    fn fill_env(&mut self, input: &EvmInput);
}

impl BlockEnvExt for BlockEnv {
    fn fill_env(&mut self, input: &EvmInput) {
        self.timestamp = input.block_timestamp.into();
        self.number = input.block_number.into();
        self.basefee = U256::ZERO;
    }
}

// -----------------------------------------------------------------------------
// Database
// -----------------------------------------------------------------------------

/// Contextual data that is read or set durint the execution of a transaction in the EVM.
struct RevmSession {
    /// Executor configuration.
    config: ExecutorConfig,

    /// Service to communicate with the storage.
    storage: Arc<StratusStorage>,

    /// Input passed to EVM to execute the transaction.
    input: EvmInput,

    /// Changes made to the storage during the execution of the transaction.
    storage_changes: ExecutionChanges,

    /// Metrics collected during EVM execution.
    metrics: EvmExecutionMetrics,
}

impl RevmSession {
    /// Creates the base session to be used with REVM.
    pub fn new(storage: Arc<StratusStorage>, config: ExecutorConfig) -> Self {
        Self {
            config,
            storage,
            input: EvmInput::default(),
            storage_changes: HashMap::default(),
            metrics: EvmExecutionMetrics::default(),
        }
    }

    /// Resets the session to be used with a new transaction.
    pub fn reset(&mut self, input: EvmInput) {
        self.input = input;
        self.storage_changes = HashMap::default();
        self.metrics = EvmExecutionMetrics::default();
    }
}

impl Database for RevmSession {
    type Error = StratusError;

    fn basic(&mut self, revm_address: RevmAddress) -> Result<Option<AccountInfo>, StratusError> {
        self.metrics.account_reads += 1;

        // retrieve account
        let address: Address = revm_address.into();
        let account = self.storage.read_account(address, self.input.point_in_time)?;

        // warn if the loaded account is the `to` account and it does not have a bytecode
        if let Some(ref to_address) = self.input.to {
            if account.bytecode.is_none() && &address == to_address && self.input.is_contract_call() {
                if self.config.executor_reject_not_contract {
                    return Err(TransactionError::AccountNotContract { address: *to_address }.into());
                } else {
                    tracing::warn!(%address, "evm to_account is not a contract because does not have bytecode");
                }
            }
        }

        // early convert response because account will be moved
        let revm_account: AccountInfo = (&account).into();

        // track original value, except if ignored address
        if not(account.address.is_ignored()) {
            self.storage_changes
                .insert(account.address, ExecutionAccountChanges::from_original_values(account));
        }

        Ok(Some(revm_account))
    }

    fn code_by_hash(&mut self, _: B256) -> Result<RevmBytecode, StratusError> {
        todo!()
    }

    fn storage(&mut self, revm_address: RevmAddress, revm_index: U256) -> Result<U256, StratusError> {
        self.metrics.slot_reads += 1;

        // convert slot
        let address: Address = revm_address.into();
        let index: SlotIndex = revm_index.into();

        // load slot from storage
        let slot = self.storage.read_slot(address, index, self.input.point_in_time)?;

        // track original value, except if ignored address
        if not(address.is_ignored()) {
            match self.storage_changes.get_mut(&address) {
                Some(account) => {
                    account.slots.insert(index, ExecutionValueChange::from_original(slot));
                }
                None => {
                    tracing::error!(reason = "reading slot without account loaded", %address, %index);
                    return Err(UnexpectedError::Unexpected(anyhow!("Account '{}' was expected to be loaded by EVM, but it was not", address)).into());
                }
            };
        }

        Ok(slot.value.into())
    }

    fn block_hash(&mut self, _: u64) -> Result<B256, StratusError> {
        todo!()
    }
}

impl DatabaseRef for RevmSession {
    type Error = StratusError;

    fn basic_ref(&self, address: revm::primitives::Address) -> Result<Option<AccountInfo>, Self::Error> {
        // retrieve account
        let address: Address = address.into();
        let account = self.storage.read_account(address, self.input.point_in_time)?;
        let revm_account: AccountInfo = (&account).into();

        Ok(Some(revm_account))
    }

    fn storage_ref(&self, address: revm::primitives::Address, index: U256) -> Result<U256, Self::Error> {
        // convert slot
        let address: Address = address.into();
        let index: SlotIndex = index.into();

        // load slot from storage
        let slot = self.storage.read_slot(address, index, self.input.point_in_time)?;

        Ok(slot.value.into())
    }

    fn block_hash_ref(&self, _: u64) -> Result<B256, Self::Error> {
        todo!()
    }

    fn code_by_hash_ref(&self, _: B256) -> Result<revm::primitives::Bytecode, Self::Error> {
        todo!()
    }
}

// -----------------------------------------------------------------------------
// Conversion
// -----------------------------------------------------------------------------

fn parse_revm_execution(revm_result: RevmResultAndState, input: EvmInput, execution_changes: ExecutionChanges) -> Result<EvmExecution, StratusError> {
    let (result, tx_output, logs, gas) = parse_revm_result(revm_result.result);
    let changes = parse_revm_state(revm_result.state, execution_changes)?;

    tracing::info!(?result, %gas, tx_output_len = %tx_output.len(), %tx_output, "evm executed");
    let mut deployed_contract_address = None;
    for changes in changes.values() {
        if changes.bytecode.is_modified() {
            deployed_contract_address = Some(changes.address);
        }
    }

    Ok(EvmExecution {
        block_timestamp: input.block_timestamp,
        receipt_applied: false,
        result,
        output: tx_output,
        logs,
        gas,
        changes,
        deployed_contract_address,
    })
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
            let output = Bytes::from(output);
            let result = ExecutionResult::Reverted { reason: (&output).into() };
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

fn parse_revm_state(revm_state: EvmState, mut execution_changes: ExecutionChanges) -> Result<ExecutionChanges, StratusError> {
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
        let (account_created, account_touched) = (revm_account.is_created(), revm_account.is_touched());

        // parse revm types to stratus primitives
        let account: Account = (revm_address, revm_account.info).into();
        let account_modified_slots: Vec<Slot> = revm_account
            .storage
            .into_iter()
            .filter_map(|(index, value)| match value.is_changed() {
                true => Some(Slot::new(index.into(), value.present_value.into())),
                false => None,
            })
            .collect();

        // handle account created (contracts) or touched (everything else)
        if account_created {
            let account_changes = ExecutionAccountChanges::from_modified_values(account, account_modified_slots);
            execution_changes.insert(account_changes.address, account_changes);
        } else if account_touched {
            let Some(account_changes) = execution_changes.get_mut(&address) else {
                tracing::error!(keys = ?execution_changes.keys(), %address, "account touched, but not loaded by evm");
                return Err(UnexpectedError::Unexpected(anyhow!("Account '{}' was expected to be loaded by EVM, but it was not", address)).into());
            };
            account_changes.apply_modifications(account, account_modified_slots);
        }
    }
    Ok(execution_changes)
}

pub fn default_trace(tracer_type: GethDebugTracerType, tx: TransactionStage) -> GethTrace {
    match tracer_type {
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::FourByteTracer) => FourByteFrame::default().into(),
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::CallTracer) => {
            let mut trace = CallFrame::default();
            trace.from = tx.from().into();
            trace.to = tx.to().map_into();
            trace.typ = "CALL".to_string();
            trace.into()
        }
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::MuxTracer) => MuxFrame::default().into(),
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::FlatCallTracer) => FlatCallFrame::default().into(),
        _ => NoopFrame::default().into(),
    }
}
