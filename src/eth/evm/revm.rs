//! Concrete Implementation of the `Evm` Trait Using `revm`
//!
//! `Revm` is a concrete implementation of the Evm trait, utilizing the `revm` library.
//! It translates the abstract requirements of Ethereum transaction execution into actionable logic,
//! interacting with the project's storage backend to manage state. `Revm` embodies the practical application
//! of the `Evm` trait, serving as a bridge between Ethereum's abstract operations and Stratus's storage mechanisms.

use std::cmp::min;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use itertools::Itertools;
use revm::primitives::AccountInfo;
use revm::primitives::Address as RevmAddress;
use revm::primitives::Bytecode as RevmBytecode;
use revm::primitives::ExecutionResult as RevmExecutionResult;
use revm::primitives::ResultAndState as RevmResultAndState;
use revm::primitives::SpecId;
use revm::primitives::State as RevmState;
use revm::primitives::TransactTo;
use revm::primitives::B256;
use revm::primitives::U256;
use revm::Database;
use revm::Evm as RevmEvm;
use revm::Handler;

use crate::eth::evm::evm::EvmExecutionResult;
use crate::eth::evm::Evm;
use crate::eth::evm::EvmError;
use crate::eth::evm::EvmInput;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::EvmExecution;
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
#[cfg(feature = "metrics")]
use crate::infra::metrics;

/// Maximum gas limit allowed for a transaction. Prevents a transaction from consuming too many resources.
const GAS_MAX_LIMIT: u64 = 1_000_000_000;

/// Implementation of EVM using [`revm`](https://crates.io/crates/revm).
pub struct Revm {
    evm: RevmEvm<'static, (), RevmSession>,
}

impl Revm {
    /// Creates a new instance of the Revm ready to be used.
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new(storage: Arc<StratusStorage>, chain_id: ChainId) -> Self {
        tracing::info!(%chain_id, "creating revm");

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
        let mut evm = RevmEvm::builder()
            .with_external_context(())
            .with_db(RevmSession::new(storage))
            .with_handler(handler)
            .build();

        // global general config
        let cfg_env = evm.cfg_mut();
        cfg_env.chain_id = chain_id.into();
        cfg_env.limit_contract_code_size = Some(usize::MAX);

        // global block config
        let block_env = evm.block_mut();
        block_env.coinbase = Address::COINBASE.into();

        // global tx config
        let tx_env = evm.tx_mut();
        tx_env.gas_priority_fee = None;

        Self { evm }
    }
}

impl Evm for Revm {
    fn execute(&mut self, input: EvmInput) -> anyhow::Result<EvmExecutionResult> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        // configure session
        let evm = &mut self.evm;
        let db = evm.db_mut();
        db.reset(input.clone());

        // if this stratus instance is the receiver of request replications we circumvent possible
        // nonce mismatches by setting it to the latest nonce of the sender.
        let nonce = if cfg!(feature = "request-replication-test-receiver") {
            db.basic(input.from.into())?.map(|acc| acc.nonce)
        } else {
            input.nonce.map_into()
        };

        // configure block params
        let block_env = evm.block_mut();
        block_env.basefee = U256::ZERO;
        block_env.timestamp = input.block_timestamp.into();
        block_env.number = input.block_number.into();

        // configure tx params
        let tx_env = &mut evm.tx_mut();
        tx_env.caller = input.from.into();
        tx_env.transact_to = match input.to {
            Some(contract) => TransactTo::Call(contract.into()),
            None => TransactTo::Create,
        };
        tx_env.gas_limit = min(input.gas_limit.into(), GAS_MAX_LIMIT);
        tx_env.gas_price = input.gas_price.into();
        tx_env.chain_id = input.chain_id.map_into();
        tx_env.nonce = nonce;
        tx_env.data = input.data.into();
        tx_env.value = input.value.into();

        // execute transaction
        let evm_result = evm.transact();

        // extract results
        let session = evm.db_mut();
        let session_input = std::mem::take(&mut session.input);
        let session_storage_changes = std::mem::take(&mut session.storage_changes);
        let session_metrics = std::mem::take(&mut session.metrics);
        #[cfg(feature = "metrics")]
        let session_point_in_time = std::mem::take(&mut session.input.point_in_time);

        // parse and enrich result
        let execution = match evm_result {
            Ok(result) => Ok(parse_revm_execution(result, session_input, session_storage_changes)),
            Err(e) => {
                tracing::warn!(reason = ?e, "evm execution error");
                Err(EvmError::Revm(e)).context("Error executing EVM transaction.")
            }
        };

        // track metrics
        #[cfg(feature = "metrics")]
        {
            metrics::inc_evm_execution(start.elapsed(), &session_point_in_time, execution.is_ok());
            metrics::inc_evm_execution_account_reads(session_metrics.account_reads);
        }

        execution.map(|execution| EvmExecutionResult {
            execution,
            metrics: session_metrics,
        })
    }
}

// -----------------------------------------------------------------------------
// Database
// -----------------------------------------------------------------------------

/// Contextual data that is read or set durint the execution of a transaction in the EVM.
struct RevmSession {
    /// Service to communicate with the storage.
    storage: Arc<StratusStorage>,

    /// Input passed to EVM to execute the transaction.
    input: EvmInput,

    /// Changes made to the storage during the execution of the transaction.
    storage_changes: ExecutionChanges,

    /// Metrics collected during EVM execution.
    metrics: ExecutionMetrics,
}

impl RevmSession {
    /// Creates the base session to be used with REVM.
    pub fn new(storage: Arc<StratusStorage>) -> Self {
        Self {
            storage,
            input: Default::default(),
            storage_changes: Default::default(),
            metrics: Default::default(),
        }
    }

    /// Resets the session to be used with a new transaction.
    pub fn reset(&mut self, input: EvmInput) {
        self.input = input;
        self.storage_changes = Default::default();
        self.metrics = Default::default();
    }
}

impl Database for RevmSession {
    type Error = anyhow::Error;

    fn basic(&mut self, revm_address: RevmAddress) -> anyhow::Result<Option<AccountInfo>> {
        self.metrics.account_reads += 1;

        // retrieve account
        let address: Address = revm_address.into();
        let account = self.storage.read_account(&address, &self.input.point_in_time)?;

        // warn if the loaded account is the `to` account and it does not have a bytecode
        if let Some(ref to_address) = self.input.to {
            if &address == to_address && not(account.is_contract()) && not(self.input.data.is_empty()) {
                tracing::warn!(%address, "evm to_account does not have bytecode");
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

    fn code_by_hash(&mut self, _: B256) -> anyhow::Result<RevmBytecode> {
        todo!()
    }

    fn storage(&mut self, revm_address: RevmAddress, revm_index: U256) -> anyhow::Result<U256> {
        self.metrics.slot_reads += 1;

        // convert slot
        let address: Address = revm_address.into();
        let index: SlotIndex = revm_index.into();

        // load slot from storage
        let slot = self.storage.read_slot(&address, &index, &self.input.point_in_time)?;

        // track original value, except if ignored address
        if not(address.is_ignored()) {
            match self.storage_changes.get_mut(&address) {
                Some(account) => {
                    account.slots.insert(index, ExecutionValueChange::from_original(slot));
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

fn parse_revm_execution(revm_result: RevmResultAndState, input: EvmInput, execution_changes: ExecutionChanges) -> EvmExecution {
    let (result, tx_output, logs, gas) = parse_revm_result(revm_result.result);
    let changes = parse_revm_state(revm_result.state, execution_changes);

    tracing::info!(?result, %gas, tx_output_len = %tx_output.len(), %tx_output, "evm executed");

    EvmExecution {
        block_timestamp: input.block_timestamp,
        receipt_applied: false,
        result,
        output: tx_output,
        logs,
        gas,
        changes,
        deployed_contract_address: None,
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
                panic!("Account '{}' was expected to be loaded by EVM, but it was not", address);
            };
            account_changes.apply_modifications(account, account_modified_slots);
        }
    }
    execution_changes
}
