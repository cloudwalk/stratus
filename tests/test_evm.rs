mod common;

use binary_macros::base16;
use ethabi::Contract;
use ethabi::Token;
use hex_literal::hex;
use ledger::eth::evm::revm::Revm;
use ledger::eth::evm::Evm;
use ledger::eth::evm::EvmCall;
use ledger::eth::evm::EvmDeployment;
use ledger::eth::evm::EvmTransaction;
use ledger::eth::primitives::Address;
use ledger::eth::primitives::Amount;
use stringreader::StringReader;

static BRLC_ABI: &str = include_str!("contracts/BRLCToken.abi");
static BRLC_BYTECODE: &[u8] = base16!("file:tests/contracts/BRLCToken.bin");

static CPP_ABI: &str = include_str!("contracts/CardPaymentProcessor.abi");
static CPP_BYTECODE: &[u8] = base16!("file:tests/contracts/CardPaymentProcessor.bin");

static PIX_ABI: &str = include_str!("contracts/PixCashier.abi");
static PIX_BYTECODE: &[u8] = base16!("file:tests/contracts/PixCashier.bin");

const DEPLOYER: Address = Address::new_const(hex!("1111111111111111111111111111111111111111"));
const MINTER: Address = Address::new_const(hex!("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"));
const TEST1: Address = Address::new_const(hex!("1111222233334444555566667777888899990000"));
const TEST2: Address = Address::new_const(hex!("0000999988887777666655554444333322221111"));

use serial_test::serial;

#[test]
#[serial]
fn evm_brlc() -> eyre::Result<()> {
    // deploy
    let (mut evm, _) = common::init_testenv();
    let (brlc, abi) = deploy_brlc(&mut evm)?;

    // execute
    log_operation("mint");
    evm.transact(EvmTransaction {
        caller: MINTER,
        contract: brlc,
        data: data!(abi.mint(TEST1, Amount::from(u16::MAX))),
    })?;

    log_operation("balance");
    evm.call(EvmCall {
        contract: brlc,
        data: data!(abi.balanceOf(TEST1)),
    })?;

    log_operation("transfer");
    evm.transact(EvmTransaction {
        caller: TEST1,
        contract: brlc,
        data: data!(abi.transfer(TEST2, Amount::from(u8::MAX))),
    })?;

    log_operation("balance");
    evm.call(EvmCall {
        contract: brlc,
        data: data!(abi.balanceOf(TEST1)),
    })?;

    log_operation("transfer too large");
    evm.transact(EvmTransaction {
        caller: TEST1,
        contract: brlc,
        data: data!(abi.transfer(TEST2, Amount::from(u32::MAX))),
    })?;

    log_operation("balance");
    evm.call(EvmCall {
        contract: brlc,
        data: data!(abi.balanceOf(TEST1)),
    })?;

    Ok(())
}

#[test]
#[serial]
fn evm_cpp() -> eyre::Result<()> {
    // deploy
    let (mut evm, _) = common::init_testenv();
    let (brlc, _) = deploy_brlc(&mut evm)?;
    let (cpp, abi) = deploy_cpp(&mut evm, brlc)?;

    // execute
    log_operation("make payment");
    evm.transact(EvmTransaction {
        caller: TEST1,
        contract: cpp,
        data: data!(abi.makePayment(Amount::ZERO, Amount::ZERO, Token::FixedBytes(vec![1u8; 16]), Token::FixedBytes(vec![1u8; 16]))),
    })?;

    log_operation("get payment");
    evm.call(EvmCall {
        contract: cpp,
        data: data!(abi.paymentFor(Token::FixedBytes(vec![1u8; 16]))),
    })?;

    Ok(())
}

#[test]
#[serial]
fn evm_pix() -> eyre::Result<()> {
    // deploy
    let (mut evm, _) = common::init_testenv();
    let (brlc, _) = deploy_brlc(&mut evm)?;
    let (pix, abi) = deploy_pix(&mut evm, brlc)?;

    // execute
    log_operation("cash-in");
    evm.transact(EvmTransaction {
        caller: MINTER,
        contract: pix,
        data: data!(abi.cashIn(TEST1, Amount::ONE, Token::FixedBytes(vec![1u8; 32]))),
    })?;

    Ok(())
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn deploy_brlc(evm: &mut Revm) -> eyre::Result<(Address, Contract)> {
    let abi = Contract::load(StringReader::new(BRLC_ABI))?;

    log_operation("deploy brlc");
    let brlc = evm.deploy(EvmDeployment {
        caller: DEPLOYER,
        data: BRLC_BYTECODE.into(),
    })?;

    log_operation("brlc owner");
    evm.call(EvmCall {
        contract: brlc,
        data: data!(abi.owner()),
    })?;

    log_operation("configure master minter");
    evm.transact(EvmTransaction {
        caller: DEPLOYER,
        contract: brlc,
        data: data!(abi.updateMasterMinter(MINTER)),
    })?;

    log_operation("configure minter");
    evm.transact(EvmTransaction {
        caller: DEPLOYER,
        contract: brlc,
        data: data!(abi.configureMinter(MINTER, Amount::from(u64::MAX))),
    })?;

    Ok((brlc, abi))
}

fn deploy_cpp(evm: &mut Revm, brlc: Address) -> eyre::Result<(Address, Contract)> {
    let abi = Contract::load(StringReader::new(CPP_ABI))?;

    log_operation("deploy cpp");
    let cpp = evm.deploy(EvmDeployment {
        caller: DEPLOYER,
        data: CPP_BYTECODE.into(),
    })?;

    log_operation("init token");
    evm.transact(EvmTransaction {
        caller: DEPLOYER,
        contract: cpp,
        data: data!(abi.initialize(brlc)),
    })?;

    Ok((cpp, abi))
}

fn deploy_pix(evm: &mut Revm, brlc: Address) -> eyre::Result<(Address, Contract)> {
    let abi = Contract::load(StringReader::new(PIX_ABI))?;

    log_operation("deploy pix");
    let pix = evm.deploy(EvmDeployment {
        caller: DEPLOYER,
        data: PIX_BYTECODE.into(),
    })?;

    log_operation("init token");
    evm.transact(EvmTransaction {
        caller: DEPLOYER,
        contract: pix,
        data: data!(abi.initialize(brlc)),
    })?;

    log_operation("grant role");
    evm.transact(EvmTransaction {
        caller: DEPLOYER,
        contract: pix,
        data: data!(abi.grantRole(
            Token::FixedBytes(hex!("ff3317e1e0e8c784290b957ee9b65ea8fe96420c4819db463283b95ecbe4a3fe").into()),
            MINTER
        )),
    })?;

    Ok((pix, abi))
}

fn log_operation(s: &str) {
    println!();
    println!("----------------------------------------");
    println!("{}", s.to_uppercase());
    println!("----------------------------------------");
    println!();
}
