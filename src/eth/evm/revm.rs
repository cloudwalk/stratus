//! Concrete Implementation of the `Evm` Trait Using `revm`
//!
//! `Revm` is a concrete implementation of the Evm trait, utilizing the `revm` library.
//! It translates the abstract requirements of Ethereum transaction execution into actionable logic,
//! interacting with the project's storage backend to manage state. `Revm` embodies the practical application
//! of the `Evm` trait, serving as a bridge between Ethereum's abstract operations and Stratus's storage mechanisms.

use std::sync::Arc;

use anyhow::anyhow;
use chrono::Utc;
use itertools::Itertools;
use revm::interpreter::InstructionResult;
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
use revm::primitives::U256;
use revm::Database;
use revm::Inspector;
use revm::EVM;
use tokio::runtime::Handle;

use crate::eth::evm::Evm;
use crate::eth::evm::EvmInput;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::ExecutionValueChange;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Log;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::storage::EthStorage;
use crate::ext::not;
use crate::ext::OptionExt;

/// Implementation of EVM using [`revm`](https://crates.io/crates/revm).
pub struct Revm {
    evm: EVM<RevmDatabaseSession>,
    storage: Arc<dyn EthStorage>,
}

impl Revm {
    /// Creates a new instance of the Revm ready to be used.
    pub fn new(storage: Arc<dyn EthStorage>) -> Self {
        let mut evm = EVM::new();

        // evm general config
        evm.env.cfg.spec_id = SpecId::LONDON;
        evm.env.cfg.limit_contract_code_size = Some(usize::MAX);
        evm.env.block.coinbase = Address::COINBASE.into();

        // evm tx config
        evm.env.tx.gas_limit = u64::MAX; // todo: must come from transaction
        evm.env.tx.gas_price = U256::ZERO;
        evm.env.tx.gas_priority_fee = None;

        Self { evm, storage }
    }
}

impl Evm for Revm {
    fn execute(&mut self, input: EvmInput) -> anyhow::Result<Execution> {
        // init session
        let evm = &mut self.evm;
        let session = RevmDatabaseSession::new(Arc::clone(&self.storage), input.point_in_time, input.to.clone());

        // configure evm block
        evm.env.block.basefee = U256::ZERO;
        evm.env.block.timestamp = U256::from(session.block_timestamp_in_secs);

        // configure database
        evm.database(session);

        // configure evm params
        let tx = &mut evm.env.tx;
        tx.caller = input.from.into();
        tx.transact_to = match input.to {
            Some(contract) => TransactTo::Call(contract.into()),
            None => TransactTo::Create(CreateScheme::Create),
        };
        tx.nonce = input.nonce.map_into();
        tx.data = input.data.into();
        tx.value = input.value.into();

        // execute evm
        #[cfg(debug_assertions)]
        let evm_result = evm.inspect(RevmInspector {});

        #[cfg(not(debug_assertions))]
        let evm_result = evm.transact();

        match evm_result {
            Ok(result) => {
                let session = evm.take_db();
                Ok(parse_revm_execution(result, session.block_timestamp_in_secs, session.storage_changes)?)
            }
            Err(e) => {
                tracing::warn!(reason = ?e, "evm execution error");
                Err(anyhow!("Error executing EVM transaction. Check logs for more information."))
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Database
// -----------------------------------------------------------------------------

/// Contextual data that is read or set durint the execution of a transaction in the EVM.
struct RevmDatabaseSession {
    /// Service to communicate with the storage.
    storage: Arc<dyn EthStorage>,

    /// Point in time of the storage during the transaction execution.
    storage_point_in_time: StoragePointInTime,

    /// Block timestamp in seconds.
    block_timestamp_in_secs: u64,

    /// Address in the `to` field.
    to: Option<Address>,

    /// Changes made to the storage during the execution of the transaction.
    storage_changes: ExecutionChanges,
}

impl RevmDatabaseSession {
    pub fn new(storage: Arc<dyn EthStorage>, storage_point_in_time: StoragePointInTime, to: Option<Address>) -> Self {
        Self {
            storage,
            storage_point_in_time,
            block_timestamp_in_secs: Utc::now().timestamp() as u64,
            to,
            storage_changes: Default::default(),
        }
    }
}

impl Database for RevmDatabaseSession {
    type Error = anyhow::Error;

    fn basic(&mut self, revm_address: RevmAddress) -> anyhow::Result<Option<AccountInfo>> {
        // retrieve account
        let address: Address = revm_address.into();
        let account = Handle::current().block_on(self.storage.read_account(&address, &self.storage_point_in_time))?;

        // warn if the loaded account is the `to` account and it does not have a bytecode
        if let Some(ref to_address) = self.to {
            if &address == to_address && not(account.is_contract()) {
                tracing::warn!(%address, "evm to_account does not have bytecode");
            }
        }

        // track original value, except if ignored address
        if not(account.address.is_ignored()) {
            self.storage_changes
                .insert(account.address.clone(), ExecutionAccountChanges::from_existing_account(account.clone()));
        }

        Ok(Some(account.into()))
    }

    fn code_by_hash(&mut self, _: B256) -> anyhow::Result<RevmBytecode> {
        todo!()
    }

    fn storage(&mut self, revm_address: RevmAddress, revm_index: U256) -> anyhow::Result<U256> {
        // retrieve slot
        let address: Address = revm_address.into();
        let index: SlotIndex = revm_index.into();
        //instructions?
        let slot = Handle::current().block_on(self.storage.read_slot(&address, &index, &self.storage_point_in_time))?;

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
// Inspector
// -----------------------------------------------------------------------------
struct RevmInspector;

impl Inspector<RevmDatabaseSession> for RevmInspector {
    fn step(&mut self, _interpreter: &mut revm::interpreter::Interpreter, _: &mut revm::EVMData<'_, RevmDatabaseSession>) -> InstructionResult {
        // let arg1 = unsafe { *interpreter.instruction_pointer.add(1) };
        // let arg2 = unsafe { *interpreter.instruction_pointer.add(2) };
        // println!(
        //     "{:02x} {:<9} {:<4x} {:<4x} {:?}",
        //     interpreter.current_opcode(),
        //     opcode::OPCODE_JUMPMAP[interpreter.current_opcode() as usize].unwrap(),
        //     arg1,
        //     arg2,
        //     interpreter.stack.data(),
        // );
        // use revm::interpreter::opcode;
        // match opcode::OPCODE_JUMPMAP[_interpreter.current_opcode() as usize] {
        //     Some(opcode) => println!("{} ", opcode),
        //     None => println!("{:#x} ", _interpreter.current_opcode() as usize),
        // }
        InstructionResult::Continue
    }
}

// -----------------------------------------------------------------------------
// Conversion
// -----------------------------------------------------------------------------

fn parse_revm_execution(
    revm_result: RevmResultAndState,
    execution_block_timestamp_in_secs: u64,
    execution_changes: ExecutionChanges,
) -> anyhow::Result<Execution> {
    let (result, output, logs, gas) = parse_revm_result(revm_result.result);
    let execution_changes = parse_revm_state(revm_result.state, execution_changes)?;

    tracing::info!(%result, %gas, output_len = %output.len(), %output, "evm executed");
    Ok(Execution {
        result,
        output,
        logs,
        gas,
        block_timestamp_in_secs: execution_block_timestamp_in_secs,
        changes: execution_changes.into_values().collect(),
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

fn parse_revm_state(revm_state: RevmState, mut execution_changes: ExecutionChanges) -> anyhow::Result<ExecutionChanges> {
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
                tracing::error!(keys = ?execution_changes.keys(), reason = "account was updated, but it was not loaded by evm", %address);
                return Err(anyhow!("Account '{}' was expected to be loaded by EVM, but it was not", address));
            };
            existing_account.apply_changes(account, account_modified_slots);
        }
    }
    Ok(execution_changes)
}

#[cfg(test)]
mod tests {
    use super::*; use ethers_core::rand::thread_rng;
    // Adjust this import to include your Revm and related structs
    use fake::{Faker, Dummy};
    use nonempty::nonempty;
    use crate::config::StorageConfig;
    use crate::eth::EthExecutor;
    use crate::eth::primitives::{TransactionInput, Wei};
    use crate::eth::storage::test_accounts;

    #[tokio::test]
    async fn regression_test_for_gas_handling() {
        let storage: StorageConfig = "inmemory".parse().unwrap(); //XXX we need to use a real storage
        let storage = storage.init().await.unwrap();
        let mut rng = thread_rng();
        let mut fake_transaction_input = TransactionInput::dummy_with_rng(&Faker, &mut rng);
        fake_transaction_input.nonce = 0.into();
        fake_transaction_input.gas_price = 0.into();
        fake_transaction_input.gas = 0.into();

        let accounts = test_accounts();
        let _ = storage.enable_test_accounts(accounts.clone()).await.unwrap();

        let address = accounts.last().unwrap().address.clone();
        fake_transaction_input.from = address;
        fake_transaction_input.value = Wei::from(0u64);

        let revm = Box::new(Revm::new(Arc::clone(&storage)));
        let executor = EthExecutor::new(nonempty![revm], Arc::clone(&storage));

        executor.transact(fake_transaction_input).await.unwrap();
    }
}
