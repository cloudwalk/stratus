use revm::interpreter::opcode;
use revm::interpreter::InstructionResult;
use revm::primitives::AccountInfo;
use revm::primitives::Address;
use revm::primitives::Bytecode;
use revm::primitives::TransactTo;
use revm::primitives::B256;
use revm::primitives::U256;
use revm::Database;
use revm::Inspector;
use revm::EVM;

use super::data;
use super::hash::keccak256;

#[derive(Debug, thiserror::Error)]
enum RevmError {
    #[error("Not found")]
    NotFound,
}
struct RevmDb {}

struct RevmDbInspector;

impl Inspector<RevmDb> for RevmDbInspector {
    fn step(&mut self, interpreter: &mut revm::interpreter::Interpreter, _: &mut revm::EVMData<'_, RevmDb>) -> InstructionResult {
        let arg1 = unsafe { *interpreter.instruction_pointer.add(1) };
        let arg2 = unsafe { *interpreter.instruction_pointer.add(2) };
        println!(
            "{:02x} {:<9} {:<4x} {:<4x} {:?}",
            interpreter.current_opcode(),
            opcode::OPCODE_JUMPMAP[interpreter.current_opcode() as usize].unwrap(),
            arg1,
            arg2,
            interpreter.stack.data(),
        );
        InstructionResult::Continue
    }
}

impl Database for RevmDb {
    type Error = RevmError;

    fn basic(&mut self, address: Address) -> Result<Option<revm::primitives::AccountInfo>, Self::Error> {
        println!();
        println!("Account | address={}", address);

        if address == Address::ZERO {
            println!("-> ZERO");
            return Ok(Some(Default::default()));
        }
        if address.0 == data::TRANSFER_ORIGIN {
            println!("-> ORIGIN");
            return Ok(Some(Default::default()));
        }
        if address.0 == data::TRANSFER_DESTINATION {
            println!("-> DESTINATION");
            return Ok(Some(Default::default()));
        }

        if address.0 == data::CONTRACT_BRLC {
            println!("-> BRLC");
            return Ok(Some(AccountInfo {
                code: Some(Bytecode::new_raw(data::bytecode_brlc_runtime().into())),
                ..Default::default()
            }));
        }

        println!("-> UNKNOWN");
        Ok(None)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        println!("Code | hash={}", code_hash);
        Err(RevmError::NotFound)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        println!();
        println!("Storage | address={} index_dec={} index_hex={:?}", address, index, index);

        if index == U256::from(0x65) {
            println!("-> PAUSED");
            return Ok(U256::ZERO);
        }

        let mut to_hash = [0; 64];
        to_hash[12..32].copy_from_slice(&data::TRANSFER_ORIGIN);
        to_hash[63] = 0x99;
        let hashed = keccak256(&to_hash);
        if index == U256::from_be_bytes(hashed) {
            println!("-> BLACKLISTED");
            return Ok(U256::MAX);
        }

        Ok(U256::ZERO)
    }

    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        println!("Block | number={}", number);
        Err(RevmError::NotFound)
    }
}

pub fn exec() {
    let mut evm: EVM<RevmDb> = EVM::new();

    // evm global config
    evm.env.cfg.limit_contract_code_size = Some(usize::MAX);

    // evm transaction config
    evm.env.tx.caller = data::TRANSFER_ORIGIN.into();
    evm.env.tx.nonce = Some(0);
    evm.env.tx.value = U256::ZERO;
    evm.env.tx.gas_price = U256::ZERO;
    evm.env.tx.gas_limit = 10_000_000;

    // function call
    evm.env.tx.transact_to = TransactTo::Call(data::CONTRACT_BRLC.into());
    evm.env.tx.data = data::TRASNFER_DATA.into();
    // evm.env.tx.data = data::SIMPLE_DATA_INC.into();
    // evm.env.tx.data = data::SIMPLE_DATA_READ.into();

    // contract creation
    // evm.env.tx.transact_to = TransactTo::Create(revm::primitives::CreateScheme::Create);
    // evm.env.tx.data = data::bytecode_brlc().into();

    evm.database(RevmDb {});
    let inspector = RevmDbInspector;
    let res = evm.inspect(inspector).unwrap();
    println!();
    println!("Executed");
    println!("-> Result = {:?}", res.result);
    // println!("-> State = {:?}", res.state);
}
