//! EVM implementation using [`revm`](https://crates.io/crates/revm).

use std::sync::Arc;

use chrono::Utc;
use revm::interpreter::InstructionResult;
use revm::primitives::AccountInfo;
use revm::primitives::Address as RevmAddress;
use revm::primitives::Bytecode as RevmBytecode;
use revm::primitives::CreateScheme;
use revm::primitives::SpecId;
use revm::primitives::TransactTo;
use revm::primitives::B256;
use revm::primitives::U256;
use revm::Database;
use revm::Inspector;
use revm::EVM;

use crate::eth::evm::Evm;
use crate::eth::evm::EvmInput;
use crate::eth::primitives::Address;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExecutionValueChange;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragerPointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;
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
        evm.env.tx.gas_price = U256::ZERO;
        evm.env.tx.gas_limit = 100_000_000;

        Self { evm, storage }
    }
}

impl Evm for Revm {
    fn execute(&mut self, input: EvmInput) -> Result<TransactionExecution, EthError> {
        // init session
        let evm = &mut self.evm;
        let session = RevmDatabaseSession::new(Arc::clone(&self.storage), input.point_in_time, input.to.clone());

        // configure evm block
        evm.env.block.timestamp = U256::from(session.block_timestamp_in_secs);

        // configure database
        evm.database(session);

        // configure evm params
        evm.env.tx.caller = input.from.into();
        evm.env.tx.transact_to = match input.to {
            Some(contract) => TransactTo::Call(contract.into()),
            None => TransactTo::Create(CreateScheme::Create),
        };
        evm.env.tx.nonce = input.nonce.map_into();
        evm.env.tx.data = input.data.into();

        // execute evm
        #[cfg(debug_assertions)]
        let evm_result = evm.inspect(RevmInspector {});

        #[cfg(not(debug_assertions))]
        let evm_result = evm.transact();

        match evm_result {
            Ok(result) => {
                let session = evm.take_db();
                Ok(TransactionExecution::from_revm_result(
                    result,
                    session.block_timestamp_in_secs,
                    session.storage_changes,
                ))?
            }
            Err(e) => {
                tracing::error!(reason = ?e, "unexpected error in evm execution");
                Err(EthError::UnexpectedEvmError)
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
    storage_point_in_time: StoragerPointInTime,

    /// Block timestamp in seconds.
    block_timestamp_in_secs: u64,

    /// Address in the `to` field.
    to: Option<Address>,

    /// Changes made to the storage during the execution of the transaction.
    storage_changes: ExecutionChanges,
}

impl RevmDatabaseSession {
    pub fn new(storage: Arc<dyn EthStorage>, storage_point_in_time: StoragerPointInTime, to: Option<Address>) -> Self {
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
    type Error = EthError;

    fn basic(&mut self, revm_address: RevmAddress) -> Result<Option<AccountInfo>, Self::Error> {
        // retrieve account and convert to REVM format
        let address: Address = revm_address.into();
        let account = self.storage.read_account(&address)?;
        let revm_account: AccountInfo = account.clone().into();

        // warn if the loaded account is the `to` account and it does not have a bytecode
        if let Some(ref to_address) = self.to {
            if &address == to_address && not(account.is_contract()) {
                tracing::warn!(%address, "evm to account does not have bytecode");
            }
        }

        // track original value
        self.storage_changes
            .insert(account.address.clone(), ExecutionAccountChanges::from_existing_account(account));

        Ok(Some(revm_account))
    }

    fn code_by_hash(&mut self, _: B256) -> Result<RevmBytecode, Self::Error> {
        todo!()
    }

    fn storage(&mut self, revm_address: RevmAddress, revm_index: U256) -> Result<U256, Self::Error> {
        // retrieve slot and convert to REVM format
        let address: Address = revm_address.into();
        let index: SlotIndex = revm_index.into();

        let slot = self.storage.read_slot(&address, &index, &self.storage_point_in_time)?;
        let revm_slot: U256 = slot.value.clone().into();

        // track original value
        match self.storage_changes.get_mut(&address) {
            Some(account) => {
                account.slots.insert(index, ExecutionValueChange::from_original(slot));
            }
            None => {
                tracing::error!(reason = "reading slot without account loaded", %address, %index);
                return Err(EthError::AccountNotLoaded(address));
            }
        };

        Ok(revm_slot)
    }

    fn block_hash(&mut self, _: U256) -> Result<B256, Self::Error> {
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
