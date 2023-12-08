//! EVM implementation using [`revm`](https://crates.io/crates/revm)

use std::sync::Arc;

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
use crate::eth::primitives::TransactionExecution;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;

/// Implementation of EVM using [`revm`](https://crates.io/crates/revm).
pub struct Revm {
    evm: EVM<RevmDatabase>,
}

impl Revm {
    /// Creates a new instance of the Revm ready to be used.
    pub fn new(storage: Arc<dyn EthStorage>) -> Self {
        let mut evm = EVM::new();

        // evm general config
        evm.env.cfg.spec_id = SpecId::LONDON;
        evm.env.cfg.limit_contract_code_size = Some(usize::MAX);
        evm.env.block.coinbase = Address::COINBASE.into();

        // evm database config
        evm.database(RevmDatabase { storage });

        // evm tx config
        let mut revm = Self { evm };
        revm.reset_emv_tx();

        revm
    }

    /// Reset EVM transaction parameters in case they were changed.
    fn reset_emv_tx(&mut self) {
        self.evm.env.tx.caller = RevmAddress::ZERO;
        self.evm.env.tx.value = U256::ZERO;
        self.evm.env.tx.gas_price = U256::ZERO;
        self.evm.env.tx.gas_limit = 100_000_000;
        self.evm.env.tx.nonce = None;
    }
}

impl Evm for Revm {
    fn transact(&mut self, input: EvmInput) -> Result<TransactionExecution, EthError> {
        // configure evm params
        self.reset_emv_tx();
        self.evm.env.tx.caller = input.caller.into();
        self.evm.env.tx.transact_to = match input.contract {
            Some(contract) => TransactTo::Call(contract.into()),
            None => TransactTo::Create(CreateScheme::Create),
        };
        self.evm.env.tx.data = input.data.into();

        // execute evm
        let evm_result = self.evm.inspect(RevmInspector {});
        match evm_result {
            Ok(result) => Ok(result.into()),
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
struct RevmDatabase {
    storage: Arc<dyn EthStorage>,
}

impl Database for RevmDatabase {
    type Error = EthError;

    fn basic(&mut self, address: RevmAddress) -> Result<Option<AccountInfo>, Self::Error> {
        let account = self.storage.read_account(&address.into())?;
        Ok(Some(account.into()))
    }

    fn code_by_hash(&mut self, _: B256) -> Result<RevmBytecode, Self::Error> {
        todo!()
    }

    fn storage(&mut self, address: RevmAddress, index: U256) -> Result<U256, Self::Error> {
        let slot = self.storage.read_slot(&address.into(), &index.into())?;
        Ok(slot.present.into())
    }

    fn block_hash(&mut self, _: U256) -> Result<B256, Self::Error> {
        todo!()
    }
}

// -----------------------------------------------------------------------------
// Inspector
// -----------------------------------------------------------------------------
struct RevmInspector;

impl Inspector<RevmDatabase> for RevmInspector {
    fn step(&mut self, _interpreter: &mut revm::interpreter::Interpreter, _: &mut revm::EVMData<'_, RevmDatabase>) -> InstructionResult {
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
