mod session;
pub mod types;
mod util;

use std::sync::Arc;

use alloy_consensus::transaction::TransactionInfo;
use alloy_rpc_types_trace::geth::FourByteFrame;
use alloy_rpc_types_trace::geth::GethDebugBuiltInTracerType;
use alloy_rpc_types_trace::geth::GethDebugTracerType;
use alloy_rpc_types_trace::geth::GethTrace;
use alloy_rpc_types_trace::geth::NoopFrame;
use anyhow::anyhow;
use log::log_enabled;
use revm::Context;
use revm::Database;
use revm::ExecuteCommitEvm;
use revm::ExecuteEvm;
use revm::InspectEvm;
use revm::context::BlockEnv;
use revm::context::Evm as RevmEvm;
use revm::context::TxEnv;
use revm::context::result::EVMError;
use revm::context::result::InvalidTransaction;
use revm::database::CacheDB;
use revm::handler::EthFrame;
use revm::handler::EthPrecompiles;
use revm::handler::instructions::EthInstructions;
use revm::interpreter::interpreter::EthInterpreter;
use revm::primitives::hardfork::SpecId;
use revm_inspectors::tracing::FourByteInspector;
use revm_inspectors::tracing::MuxInspector;
use revm_inspectors::tracing::TracingInspector;
use revm_inspectors::tracing::TracingInspectorConfig;
use revm_inspectors::tracing::js::JsInspector;
use session::RevmSession;
use types::ContextWithDB;
pub use types::EvmKind;
pub use types::GeneralRevm;
use util::default_trace;
use util::enhance_trace_with_decoded_errors;
use util::parse_revm_result_and_state;

use crate::eth::executor::EvmExecutionResult;
use crate::eth::executor::EvmInput;
use crate::eth::executor::ExecutorConfig;
use crate::eth::executor::evm::types::InspectorInput;
use crate::eth::executor::evm::util::EvmExt;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::ExecutorError;
use crate::eth::primitives::MinedData;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionExecution;
use crate::eth::storage::StratusStorage;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

/// Implementation of EVM using [`revm`](https://crates.io/crates/revm).
pub struct Evm {
    evm: RevmEvm<ContextWithDB, (), EthInstructions<EthInterpreter, ContextWithDB>, EthPrecompiles, EthFrame>,
    kind: EvmKind,
}

impl Evm {
    /// Creates a new instance of the Evm.
    pub fn new(storage: Arc<StratusStorage>, config: ExecutorConfig, kind: EvmKind) -> Self {
        tracing::info!(?config, "creating revm");

        // configure revm
        let chain_id = config.executor_chain_id;

        Self {
            evm: Self::create_evm(chain_id, config.executor_evm_spec, RevmSession::new(storage, config.clone()), kind),
            kind,
        }
    }

    /// Execute a transaction that deploys a contract or call a contract function.
    pub fn execute(&mut self, input: EvmInput) -> Result<EvmExecutionResult, StratusError> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        // configure session
        self.evm.journaled_state.database.reset(input.clone());

        self.evm.fill_env(input);

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
        let session_input = std::mem::take(&mut session.input);
        let session_metrics = std::mem::take(&mut session.metrics);
        #[cfg(feature = "metrics")]
        let session_point_in_time = session.input.point_in_time;

        // parse result
        let execution = match evm_result {
            // executed
            Ok(result_and_state) => Ok(parse_revm_result_and_state(result_and_state, session_input)?),

            // nonce errors
            Err(EVMError::Transaction(InvalidTransaction::NonceTooHigh { tx, state })) => Err(ExecutorError::Nonce {
                transaction: tx.into(),
                account: state.into(),
            }
            .into()),
            Err(EVMError::Transaction(InvalidTransaction::NonceTooLow { tx, state })) => Err(ExecutorError::Nonce {
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
                Err(ExecutorError::EvmFailed(e.to_string()).into())
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

    fn create_evm<DB: Database>(chain_id: u64, spec: SpecId, db: DB, kind: EvmKind) -> GeneralRevm<DB> {
        let ctx = Context::new(db, spec)
            .modify_cfg_chained(|cfg_env| {
                cfg_env.chain_id = chain_id;
                cfg_env.spec = spec;
                cfg_env.tx_chain_id_check = kind.is_transaction();
                cfg_env.limit_contract_initcode_size = None;
                cfg_env.disable_nonce_check = kind.is_call();
                cfg_env.max_blobs_per_tx = None;
                cfg_env.tx_gas_limit_cap = None;
                cfg_env.blob_base_fee_update_fraction = None;
                cfg_env.disable_eip3607 = kind.is_call();
                cfg_env.limit_contract_code_size = Some(usize::MAX);
                cfg_env.memory_limit = (1 << 32) - 1;
                cfg_env.disable_balance_check = false;
                cfg_env.disable_block_gas_limit = false;
                cfg_env.disable_eip3541 = false;
                cfg_env.disable_eip7623 = false;
                cfg_env.disable_base_fee = false;
                cfg_env.disable_fee_charge = false;
            })
            .modify_block_chained(|block_env: &mut BlockEnv| {
                block_env.beneficiary = Address::COINBASE.into();
            })
            .modify_tx_chained(|tx_env: &mut TxEnv| {
                tx_env.gas_priority_fee = None;
            });

        RevmEvm::new(ctx, EthInstructions::new_mainnet_with_spec(spec), EthPrecompiles::new(spec))
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

        let (tx, mined_data): (TransactionExecution, Option<MinedData>) = self
            .evm
            .journaled_state
            .database
            .storage
            .read_transaction(tx_hash)?
            .ok_or_else(|| anyhow!("transaction not found: {tx_hash}"))?
            .into();

        // CREATE transactions need to be traced for blockscout to work correctly
        if tx.result.execution.deployed_contract_address.is_none() && trace_unsuccessful_only && matches!(tx.result.execution.result, ExecutionResult::Success)
        {
            return Ok(default_trace(tracer_type, tx));
        }

        let block = self
            .evm
            .journaled_state
            .database
            .storage
            .read_block(BlockFilter::Number(tx.evm_input.block_number))?
            .ok_or_else(|| {
                StratusError::Storage(StorageError::BlockNotFound {
                    filter: BlockFilter::Number(tx.evm_input.block_number),
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
        let inspect_input: EvmInput = tx.evm_input;
        self.evm.journaled_state.database.reset(EvmInput {
            point_in_time: PointInTime::MinedPast(inspect_input.block_number.prev().unwrap_or_default()),
            ..Default::default()
        });

        let spec = self.evm.cfg.spec;

        let mut cache_db = CacheDB::new(&self.evm.journaled_state.database);
        let mut evm = Self::create_evm(inspect_input.chain_id.unwrap_or_default().into(), spec, &mut cache_db, self.kind);

        // Execute all transactions before target tx_hash
        for tx in block.transactions.into_iter() {
            if tx.info.hash == tx_hash {
                break;
            }
            let tx_input: EvmInput = tx.execution.evm_input;

            // Configure EVM state
            evm.fill_env(tx_input);
            let tx = std::mem::take(&mut evm.tx);
            evm.transact_commit(tx)?;
        }

        let trace_result: GethTrace = match tracer_type {
            GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::FourByteTracer) => {
                let mut inspector = FourByteInspector::default();
                let mut evm_with_inspector = evm.with_inspector(&mut inspector);
                evm_with_inspector.fill_env(inspect_input);
                let tx = std::mem::take(&mut evm_with_inspector.tx);
                evm_with_inspector.inspect_tx(tx)?;
                FourByteFrame::from(&inspector).into()
            }
            GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::CallTracer) => {
                let call_config = opts.tracer_config.into_call_config()?;
                let mut inspector = TracingInspector::new(TracingInspectorConfig::from_geth_call_config(&call_config));
                let mut evm_with_inspector = evm.with_inspector(&mut inspector);
                evm_with_inspector.fill_env(inspect_input);
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
                evm_with_inspector.fill_env(inspect_input);
                let tx = std::mem::take(&mut evm_with_inspector.tx);
                let res = evm_with_inspector.inspect_tx(tx)?;

                inspector.geth_builder().geth_prestate_traces(&res, &prestate_config, &cache_db)?.into()
            }
            GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::NoopTracer) => NoopFrame::default().into(),
            GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::MuxTracer) => {
                let mux_config = opts.tracer_config.into_mux_config()?;
                let mut inspector = MuxInspector::try_from_config(mux_config).map_err(|e| anyhow!(e))?;
                let mut evm_with_inspector = evm.with_inspector(&mut inspector);
                evm_with_inspector.fill_env(inspect_input);
                let tx = std::mem::take(&mut evm_with_inspector.tx);
                let res = evm_with_inspector.inspect_tx(tx)?;
                inspector.try_into_mux_frame(&res, &cache_db, tx_info)?.into()
            }
            GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::FlatCallTracer) => {
                let flat_call_config = opts.tracer_config.into_flat_call_config()?;
                let mut inspector = TracingInspector::new(TracingInspectorConfig::from_flat_call_config(&flat_call_config));
                let mut evm_with_inspector = evm.with_inspector(&mut inspector);
                evm_with_inspector.fill_env(inspect_input);
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
                evm_with_inspector.fill_env(inspect_input);
                let tx = std::mem::take(&mut evm_with_inspector.tx);
                let block = std::mem::take(&mut evm_with_inspector.block);
                let res = evm_with_inspector.inspect_tx(tx.clone())?;
                GethTrace::JS(inspector.json_result(res, &tx, &block, &cache_db).map_err(|e| anyhow!(e.to_string()))?)
            }
            GethDebugTracerType::BuiltInTracer(unimplemented) => {
                return Err(anyhow!("{unimplemented:?} not implemented").into());
            }
        };

        Ok(trace_result)
    }
}
