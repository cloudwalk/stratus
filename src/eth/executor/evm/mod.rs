mod session;
pub mod types;
mod util;

use std::marker::PhantomData;
use std::sync::Arc;

use alloy_consensus::transaction::TransactionInfo;
use alloy_rpc_types_trace::geth::FourByteFrame;
use alloy_rpc_types_trace::geth::GethDebugBuiltInTracerType;
use alloy_rpc_types_trace::geth::GethDebugTracerType;
use alloy_rpc_types_trace::geth::GethDebugTracingOptions;
use alloy_rpc_types_trace::geth::GethTrace;
use alloy_rpc_types_trace::geth::NoopFrame;
use anyhow::anyhow;
use log::log_enabled;
use revm::ExecuteCommitEvm;
use revm::ExecuteEvm;
use revm::InspectEvm;
use revm::context::result::ExecResultAndState;
use revm::context::result::ExecutionResult as RevmExecResult;
use revm::database::CacheDB;
use revm::primitives::hardfork::SpecId;
use revm_inspectors::tracing::FourByteInspector;
use revm_inspectors::tracing::MuxInspector;
use revm_inspectors::tracing::TracingInspector;
use revm_inspectors::tracing::TracingInspectorConfig;
use revm_inspectors::tracing::js::JsInspector;
use session::RevmSession;
pub use types::EvmKind;
pub use types::GeneralRevm;
use util::default_trace;
use util::enhance_trace_with_decoded_errors;

use crate::eth::executor::EvmExecutionMetrics;
use crate::eth::executor::ExecutionResult;
use crate::eth::executor::ExecutorConfig;
use crate::eth::executor::TransactionExecution;
use crate::eth::executor::TransactionExecutionInput;
use crate::eth::executor::evm::types::CallExecutionInput;
use crate::eth::executor::evm::types::EvmInput;
use crate::eth::executor::evm::types::InspectorInput;
use crate::eth::executor::evm::util::EvmExt;
use crate::eth::executor::evm::util::create_evm;
use crate::eth::rpc::BlockFilter;
use crate::eth::rpc::RpcError;
use crate::eth::storage::ExecutionKind;
use crate::eth::storage::StorageError;
use crate::eth::storage::StratusStorage;
use crate::eth::types::CallInput;
use crate::eth::types::Hash;
use crate::eth::types::MinedData;
use crate::eth::types::PointInTime;
use crate::eth::types::StratusError;

pub type RevmResultAndState = ExecResultAndState<RevmExecResult>;

/// Implementation of EVM using [`revm`](https://crates.io/crates/revm).
pub struct Evm<Input: EvmInput> {
    evm: GeneralRevm<RevmSession>,
    kind: EvmKind,
    _input_type: PhantomData<Input>,
}

impl<Input: EvmInput> Evm<Input> {
    /// Creates a new instance of the Evm.
    pub fn new(storage: Arc<StratusStorage>, config: &ExecutorConfig, kind: EvmKind) -> Self {
        tracing::info!(?config, "creating revm");

        // configure revm
        let chain_id = config.executor_chain_id;

        Self {
            evm: create_evm(chain_id, config.executor_evm_spec, RevmSession::new(storage), kind),
            kind,
            _input_type: PhantomData,
        }
    }

    /// Execute a transaction that deploys a contract or call a contract function.
    pub fn execute(&mut self, input: Input) -> Result<(RevmResultAndState, EvmExecutionMetrics), StratusError> {
        // configure session
        self.evm.journaled_state.database.reset(input.kind());
        input.fill_env(&mut self.evm);

        if log_enabled!(log::Level::Debug) {
            let block_env_log = self.evm.block.clone();
            let tx_env_log = self.evm.tx.clone();
            // execute transaction
            tracing::debug!(block_env = ?block_env_log, tx_env = ?tx_env_log, "executing transaction in revm");
        }

        let tx = std::mem::take(&mut self.evm.tx);
        let evm_result = self.evm.transact(tx);

        // extract results
        let session = &mut self.evm.journaled_state.database;
        let slot_access_metrics = std::mem::take(&mut session.metrics);

        evm_result
            .inspect_err(|err| tracing::warn!(?err, "evm error"))
            .map_err(|err| err.into())
            .map(|execution| {
                let gas_used = (*execution.result.gas()).into();
                let metrics = EvmExecutionMetrics {
                    slot_access: slot_access_metrics,
                    gas_used,
                };
                (execution, metrics)
            })
    }
}

impl Evm<TransactionExecutionInput> {
    /// Execute a transaction or a synthetic call using a tracer.
    pub fn inspect(&mut self, input: InspectorInput) -> Result<GethTrace, StratusError> {
        match input {
            InspectorInput::Transaction {
                tx_hash,
                opts,
                trace_unsuccessful_only,
            } => self.inspect_transaction(tx_hash, opts, trace_unsuccessful_only),
            InspectorInput::Call { call, point_in_time, opts } => self.inspect_call(call, point_in_time, opts),
        }
    }

    /// Re-executes an already-mined transaction, looked up by hash, wrapped in the requested tracer.
    ///
    /// Because Stratus only stores state at block boundaries, every transaction before the target one in the
    /// same block is replayed first, to reconstruct the exact mid-block state the target transaction saw.
    fn inspect_transaction(&mut self, tx_hash: Hash, opts: GethDebugTracingOptions, trace_unsuccessful_only: bool) -> Result<GethTrace, StratusError> {
        let tracer_type = opts.tracer.clone().ok_or_else(|| anyhow!("no tracer type provided"))?;

        if matches!(tracer_type, GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::NoopTracer)) {
            return Ok(NoopFrame::default().into());
        }

        let (tx, mined_data): (TransactionExecution, Option<MinedData>) = self
            .evm
            .journaled_state
            .database
            .storage
            .read_transaction(tx_hash)?
            .ok_or_else(|| anyhow!("transaction not found: {tx_hash}"))?
            .into();

        // CREATE transactions need to be traced for blockscout to work correctly
        if tx.output.deployed_contract_address.is_none() && trace_unsuccessful_only && matches!(tx.output.result, ExecutionResult::Success) {
            return Ok(default_trace(tracer_type, tx));
        }

        let block = self
            .evm
            .journaled_state
            .database
            .storage
            .read_block(BlockFilter::Number(tx.input.block_number))?
            .ok_or_else(|| {
                StratusError::Storage(StorageError::BlockNotFound {
                    filter: BlockFilter::Number(tx.input.block_number),
                })
            })?;

        let tx_info = TransactionInfo {
            block_hash: Some(block.hash().0.0.into()),
            block_timestamp: Some(*block.timestamp()),
            hash: Some(tx_hash.0.0.into()),
            index: mined_data.map(|data| data.index.into()),
            block_number: Some(block.number().as_u64()),
            base_fee: None,
        };
        let inspect_input: TransactionExecutionInput = tx.input;
        let target = inspect_input.block_number.prev().unwrap_or_default();
        self.evm.journaled_state.database.reset(ExecutionKind::CallPast(target));

        let spec = self.evm.cfg.spec;
        let chain_id: u64 = inspect_input.chain_id.unwrap_or_default().into();

        let mut cache_db = CacheDB::new(&self.evm.journaled_state.database);
        let mut evm = create_evm(chain_id, spec, &mut cache_db, self.kind);

        // Execute all transactions before target tx_hash, to reconstruct the mid-block state it saw.
        for tx in block.transactions.into_iter() {
            if tx.info.hash == tx_hash {
                break;
            }
            let tx_input: TransactionExecutionInput = tx.execution.input;

            // Configure EVM state
            evm.fill_env(tx_input);
            let tx = std::mem::take(&mut evm.tx);
            evm.transact_commit(tx)?;
        }

        run_tracer(tracer_type, opts, spec, chain_id, self.kind, cache_db, inspect_input, tx_info)
    }

    /// Executes a synthetic call that was never signed or broadcast, wrapped in the requested tracer, against a
    /// chosen point in time. Unlike [`Self::inspect_transaction`], there is no real position in a block to
    /// reconstruct, so the call runs directly against the resolved state boundary — same as [`Self::execute`]
    /// does for `eth_call`, just with a tracer attached.
    fn inspect_call(&mut self, call: CallInput, point_in_time: PointInTime, opts: GethDebugTracingOptions) -> Result<GethTrace, StratusError> {
        let tracer_type = opts.tracer.clone().ok_or_else(|| anyhow!("no tracer type provided"))?;

        if matches!(tracer_type, GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::NoopTracer)) {
            return Ok(NoopFrame::default().into());
        }

        let inspect_input: CallExecutionInput = match point_in_time {
            PointInTime::Pending => {
                let (pending_header, tx_count) = self.evm.journaled_state.database.storage.read_pending_block_header();
                CallExecutionInput::from_pending_block(call, pending_header, tx_count)
            }
            point_in_time => {
                let Some(block) = self.evm.journaled_state.database.storage.read_block(point_in_time.into())? else {
                    return Err(RpcError::BlockFilterInvalid { filter: point_in_time.into() }.into());
                };
                CallExecutionInput::try_from_mined_block(call, block, point_in_time)?
            }
        };

        let tx_info = TransactionInfo {
            block_hash: None,
            block_timestamp: Some(*inspect_input.block_timestamp),
            hash: None,
            index: None,
            block_number: Some(inspect_input.block_number.as_u64()),
            base_fee: None,
        };

        // point the base session at the resolved state boundary — no replay needed, this is the only execution
        self.evm.journaled_state.database.reset(inspect_input.kind());

        let spec = self.evm.cfg.spec;
        let chain_id = self.evm.cfg.chain_id;
        let cache_db = CacheDB::new(&self.evm.journaled_state.database);

        run_tracer(tracer_type, opts, spec, chain_id, self.kind, cache_db, inspect_input, tx_info)
    }
}

/// Runs `inspect_input` through the EVM wrapped in the inspector implied by `tracer_type`, against `cache_db`.
///
/// Shared by [`Evm::inspect_transaction`] (where `cache_db` already has the preceding transactions of the block
/// committed into it) and [`Evm::inspect_call`] (where `cache_db` is empty and reads fall straight through to
/// the underlying session).
#[allow(clippy::too_many_arguments)]
fn run_tracer<Input: EvmInput>(
    tracer_type: GethDebugTracerType,
    opts: GethDebugTracingOptions,
    spec: SpecId,
    chain_id: u64,
    kind: EvmKind,
    mut cache_db: CacheDB<&RevmSession>,
    inspect_input: Input,
    tx_info: TransactionInfo,
) -> Result<GethTrace, StratusError> {
    let evm = create_evm(chain_id, spec, &mut cache_db, kind);

    let trace_result: GethTrace = match tracer_type {
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::FourByteTracer) => {
            let mut inspector = FourByteInspector::default();
            let mut evm_with_inspector = evm.with_inspector(&mut inspector);
            inspect_input.fill_env(&mut evm_with_inspector);
            let tx = std::mem::take(&mut evm_with_inspector.tx);
            evm_with_inspector.inspect_tx(tx)?;
            FourByteFrame::from(&inspector).into()
        }
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::CallTracer) => {
            let call_config = opts.tracer_config.into_call_config()?;
            let mut inspector = TracingInspector::new(TracingInspectorConfig::from_geth_call_config(&call_config));
            let mut evm_with_inspector = evm.with_inspector(&mut inspector);
            inspect_input.fill_env(&mut evm_with_inspector);
            let tx = std::mem::take(&mut evm_with_inspector.tx);
            let res = evm_with_inspector.inspect_tx(tx)?;
            let mut trace = inspector.geth_builder().geth_call_traces(call_config, res.result.tx_gas_used()).into();
            enhance_trace_with_decoded_errors(&mut trace);
            trace
        }
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::PreStateTracer) => {
            let prestate_config = opts.tracer_config.into_pre_state_config()?;
            let mut inspector = TracingInspector::new(TracingInspectorConfig::from_geth_prestate_config(&prestate_config));
            let mut evm_with_inspector = evm.with_inspector(&mut inspector);
            inspect_input.fill_env(&mut evm_with_inspector);
            let tx = std::mem::take(&mut evm_with_inspector.tx);
            let res = evm_with_inspector.inspect_tx(tx)?;

            inspector.geth_builder().geth_prestate_traces(&res, &prestate_config, &cache_db)?.into()
        }
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::NoopTracer) => NoopFrame::default().into(),
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::MuxTracer) => {
            let mux_config = opts.tracer_config.into_mux_config()?;
            let mut inspector = MuxInspector::try_from_config(mux_config).map_err(|e| anyhow!(e))?;
            let mut evm_with_inspector = evm.with_inspector(&mut inspector);
            inspect_input.fill_env(&mut evm_with_inspector);
            let tx = std::mem::take(&mut evm_with_inspector.tx);
            let res = evm_with_inspector.inspect_tx(tx)?;
            inspector.try_into_mux_frame(&res, &cache_db, tx_info)?.into()
        }
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::FlatCallTracer) => {
            let flat_call_config = opts.tracer_config.into_flat_call_config()?;
            let mut inspector = TracingInspector::new(TracingInspectorConfig::from_flat_call_config(&flat_call_config));
            let mut evm_with_inspector = evm.with_inspector(&mut inspector);
            inspect_input.fill_env(&mut evm_with_inspector);
            let tx = std::mem::take(&mut evm_with_inspector.tx);
            let res = evm_with_inspector.inspect_tx(tx)?;
            inspector
                .with_transaction_gas_limit(res.result.tx_gas_used())
                .into_parity_builder()
                .into_localized_transaction_traces(tx_info)
                .into()
        }
        GethDebugTracerType::JsTracer(code) => {
            let mut inspector = JsInspector::new(code, opts.tracer_config.into_json()).map_err(|e| anyhow!(e.to_string()))?;
            let mut evm_with_inspector = evm.with_inspector(&mut inspector);
            inspect_input.fill_env(&mut evm_with_inspector);
            let tx = std::mem::take(&mut evm_with_inspector.tx);
            let block = std::mem::take(&mut evm_with_inspector.block);
            let res = evm_with_inspector.inspect_tx(tx.clone())?;
            GethTrace::JS(inspector.json_result(res, &tx, &block, &cache_db).map_err(|e| anyhow!(e.to_string()))?)
        }
        GethDebugTracerType::BuiltInTracer(tracer) => {
            return Err(anyhow!("tracer {tracer:?} is not implemented").into());
        }
    };

    Ok(trace_result)
}
