mod config;
mod evm;
mod evm_worker_pool;
mod transaction_worker;
pub mod types;

use std::sync::Arc;

#[cfg(feature = "metrics")]
use alloy_rpc_types_trace::geth::GethDebugTracerType;
use alloy_rpc_types_trace::geth::GethDebugTracingOptions;
use alloy_rpc_types_trace::geth::GethTrace;
pub use config::ExecutorConfig;
pub use evm::types::AccessListOutput;
pub use evm::types::CallExecutionOutput;
pub use evm::types::EvmKind;
pub use evm::types::ExecutionMetrics;
pub use evm::types::TransactionExecutionInput;
pub use evm::types::TransactionExecutionOutput;
pub use evm::types::TransactionExecutionResult;
use stratus_metrics::timed;
use tracing::Span;
#[cfg(feature = "tracing")]
use tracing::info_span;
pub use types::ExecutionResult;
pub use types::ExecutorError;
pub use types::RevertReason;
pub use types::State;
pub use types::TransactionExecution;

#[cfg(feature = "metrics")]
use crate::eth::codegen;
use crate::eth::executor::evm::RevmResultAndState;
use crate::eth::executor::evm::types::CallExecutionInput;
use crate::eth::executor::evm::types::InspectorInput;
use crate::eth::executor::evm_worker_pool::EvmWorkerPool;
use crate::eth::executor::transaction_worker::TransactionWorker;
use crate::eth::executor::types::EvmRoute;
use crate::eth::miner::Miner;
use crate::eth::storage::ExecutionKind;
use crate::eth::storage::StorageError;
use crate::eth::storage::StratusStorage;
use crate::eth::types::Address;
use crate::eth::types::Block;
use crate::eth::types::CallInput;
use crate::eth::types::Hash;
use crate::eth::types::PointInTime;
use crate::eth::types::StratusError;
use crate::eth::types::TransactionInput;
use crate::infra::tracing::SpanExt;
use crate::utils::Semaphore;

// -----------------------------------------------------------------------------
// Executor
// -----------------------------------------------------------------------------

pub struct Executor {
    /// Limits concurrent transaction warmup and queueing.
    transaction_warmup: Semaphore,

    /// Serial worker that executes and persists transactions.
    transaction_worker: TransactionWorker,

    /// Channels to send calls and inspections to background EVMs.
    evms: EvmWorkerPool,

    /// Shared storage backend for persisting blockchain state.
    storage: Arc<StratusStorage>,

    /// Whether to reject transactions and calls targeting accounts that are not contracts.
    reject_not_contract: bool,
}

impl Executor {
    pub fn new(storage: Arc<StratusStorage>, miner: Arc<Miner>, config: ExecutorConfig) -> Self {
        tracing::info!(?config, "creating executor");
        let reject_not_contract = config.executor_reject_not_contract;
        let transaction_worker = TransactionWorker::spawn(Arc::clone(&storage), Arc::clone(&miner), &config);
        let evms = EvmWorkerPool::spawn(Arc::clone(&storage), &config);
        Self {
            transaction_warmup: Semaphore::new(100),
            transaction_worker,
            evms,
            storage,
            reject_not_contract,
        }
    }

    // -------------------------------------------------------------------------
    // Imported transactions
    // -------------------------------------------------------------------------

    /// Reexecutes an imported block locally, importing it to the temporary storage.
    pub fn execute_imported_block(&self, block: &Block) -> Result<(), StratusError> {
        #[cfg(feature = "tracing")]
        let _span = info_span!("executor::imported_block", block_number = %block.number()).entered();
        tracing::info!(block_number = %block.number(), "reexecuting imported block");

        self.storage.set_pending_header(block.number(), block.timestamp());

        for tx in &block.transactions {
            let tx_input: TransactionInput = (&tx.execution).into();
            self.execute_local_transaction(tx_input, None)?;
        }

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Local transactions
    // -------------------------------------------------------------------------

    /// Validates that the target account is a contract, reading it from storage at the given point in time.
    pub fn validate_to_is_contract(&self, to_address: Address, kind: ExecutionKind) -> Result<(), StratusError> {
        let (account, _) = self.storage.read_account(to_address, kind)?;
        if account.bytecode.is_none() {
            if self.reject_not_contract {
                return Err(ExecutorError::AccountNotContract { address: to_address }.into());
            } else {
                tracing::warn!(%to_address, "evm to_account is not a contract because does not have bytecode");
            }
        }
        Ok(())
    }

    /// Executes a transaction persisting state changes.
    #[tracing::instrument(name = "executor::local_transaction", skip_all, fields(tx_hash, tx_from, tx_to, tx_nonce))]
    pub fn execute_local_transaction(&self, tx: TransactionInput, access_list: Option<AccessListOutput>) -> Result<(), StratusError> {
        if tx.signer().is_zero() {
            return Err(ExecutorError::FromZeroAddress.into());
        }

        // track
        Span::with(|s| {
            s.rec_str("tx_hash", &tx.transaction_info.hash);
            s.rec_str("tx_from", &tx.signer());
            s.rec_opt("tx_to", &tx.execution_info.to);
            s.rec_str("tx_nonce", &tx.execution_info.nonce);
        });

        let _permit = self.transaction_warmup.acquire();

        if let Some(access_list) = access_list {
            self.storage.load_access_list(access_list);
        }

        // The transaction worker serializes execution and persistence so that the next transaction
        // cannot execute against state that the previous transaction has not saved yet.
        self.transaction_worker.execute_local_transaction(tx).map(|_| ())
    }

    /// Executes a read-only call in the local EVM, without persisting state changes.
    #[tracing::instrument(name = "executor::local_call", skip_all, fields(from, to))]
    #[timed(executor_local_call, labels(
        success = result.is_ok(),
        contract = |call_input| codegen::contract_name(&call_input.to),
        function = |call_input| codegen::function_sig(&call_input.data),
        kind = |kind| kind.as_ref()
        )
    )]
    pub fn execute_local_call<Output>(&self, call_input: CallInput, kind: ExecutionKind) -> Result<Output, StratusError>
    where
        Output: TryFrom<RevmResultAndState, Error = StratusError>,
    {
        Span::with(|s| {
            s.rec_opt("from", &call_input.from);
            s.rec_opt("to", &call_input.to);
        });

        let filter = kind.into();
        let block_info_opt = if matches!(kind, ExecutionKind::AccessList) {
            Some(self.storage.read_latest_block_info_relaxed())
        } else {
            self.storage.read_block_info(filter)?
        };
        let Some(block_info) = block_info_opt else {
            return Err(StorageError::BlockNotFound { filter }.into());
        };

        let evm_input = CallExecutionInput::create(call_input, block_info, kind);

        let evm_route = match kind.point_in_time() {
            PointInTime::Pending | PointInTime::Latest => EvmRoute::CallPresent(evm_input),
            PointInTime::Past(_) => EvmRoute::CallPast(evm_input),
        };

        self.evms.execute::<Output>(evm_route).map(|(output, _metrics)| output)
    }

    #[timed(executor_inspect, labels(
        trace_type = |opts|
            opts
            .as_ref()
            .and_then(|opts| opts.tracer.as_ref())
            .map_or("missing", GethDebugTracerType::as_str)
        )
    )]
    pub fn trace_transaction(&self, tx_hash: Hash, opts: Option<GethDebugTracingOptions>, trace_unsuccessful_only: bool) -> Result<GethTrace, StratusError> {
        Span::with(|s| {
            s.rec_str("tx_hash", &tx_hash);
        });

        self.evms.inspect(InspectorInput {
            tx_hash,
            opts: opts.unwrap_or_default(),
            trace_unsuccessful_only,
        })
    }
}
