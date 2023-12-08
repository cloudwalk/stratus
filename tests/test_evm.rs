mod common;

use binary_macros::base16;
use ethabi::Contract;
use ethabi::Token;
use hex_literal::hex;
use ledger::eth::primitives::Address;
use ledger::eth::primitives::Amount;
use ledger::eth::EthCall;
use ledger::eth::EthDeployment;
use ledger::eth::EthExecutor;
use ledger::eth::EthTransaction;
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
    let executor = common::init_testenv();
    let (brlc, abi) = deploy_brlc(&executor)?;

    // execute
    log_operation("mint");
    executor.transact(EthTransaction {
        caller: MINTER,
        contract: brlc.clone(),
        data: data!(abi.mint(TEST1, Amount::from(u16::MAX))),
        ..Default::default()
    })?;

    log_operation("balance");
    executor.call(EthCall {
        contract: brlc.clone(),
        data: data!(abi.balanceOf(TEST1)),
    })?;

    log_operation("transfer");
    executor.transact(EthTransaction {
        caller: TEST1,
        contract: brlc.clone(),
        data: data!(abi.transfer(TEST2, Amount::from(u8::MAX))),
        ..Default::default()
    })?;

    log_operation("balance");
    executor.call(EthCall {
        contract: brlc.clone(),
        data: data!(abi.balanceOf(TEST1)),
    })?;

    log_operation("transfer too large");
    executor.transact(EthTransaction {
        caller: TEST1,
        contract: brlc.clone(),
        data: data!(abi.transfer(TEST2, Amount::from(u32::MAX))),
        ..Default::default()
    })?;

    log_operation("balance");
    executor.call(EthCall {
        contract: brlc,
        data: data!(abi.balanceOf(TEST1)),
    })?;

    Ok(())
}

#[test]
#[serial]
fn evm_cpp() -> eyre::Result<()> {
    // deploy
    let executor = common::init_testenv();
    let (brlc, _) = deploy_brlc(&executor)?;
    let (cpp, abi) = deploy_cpp(&executor, brlc.clone())?;

    // execute
    log_operation("make payment");
    executor.transact(EthTransaction {
        caller: TEST1,
        contract: cpp.clone(),
        data: data!(abi.makePayment(Amount::ZERO, Amount::ZERO, Token::FixedBytes(vec![1u8; 16]), Token::FixedBytes(vec![1u8; 16]))),
        ..Default::default()
    })?;

    log_operation("get payment");
    executor.call(EthCall {
        contract: cpp.clone(),
        data: data!(abi.paymentFor(Token::FixedBytes(vec![1u8; 16]))),
    })?;

    Ok(())
}

#[test]
#[serial]
fn evm_pix() -> eyre::Result<()> {
    // deploy
    let executor = common::init_testenv();
    let (brlc, _) = deploy_brlc(&executor)?;
    let (pix, abi) = deploy_pix(&executor, brlc)?;

    // execute
    log_operation("cash-in");
    executor.transact(EthTransaction {
        caller: MINTER,
        contract: pix,
        data: data!(abi.cashIn(TEST1, Amount::ONE, Token::FixedBytes(vec![1u8; 32]))),
        ..Default::default()
    })?;

    Ok(())
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn deploy_brlc(evm: &EthExecutor) -> eyre::Result<(Address, Contract)> {
    let abi = Contract::load(StringReader::new(BRLC_ABI))?;

    log_operation("deploy brlc");
    let brlc = evm.deploy(EthDeployment {
        caller: DEPLOYER,
        data: BRLC_BYTECODE.into(),
        ..Default::default()
    })?;

    log_operation("brlc owner");
    evm.call(EthCall {
        contract: brlc.clone(),
        data: data!(abi.owner()),
    })?;

    log_operation("configure master minter");
    evm.transact(EthTransaction {
        caller: DEPLOYER,
        contract: brlc.clone(),
        data: data!(abi.updateMasterMinter(MINTER)),
        ..Default::default()
    })?;

    log_operation("configure minter");
    evm.transact(EthTransaction {
        caller: DEPLOYER,
        contract: brlc.clone(),
        data: data!(abi.configureMinter(MINTER, Amount::from(u64::MAX))),
        ..Default::default()
    })?;

    Ok((brlc, abi))
}

fn deploy_cpp(evm: &EthExecutor, brlc: Address) -> eyre::Result<(Address, Contract)> {
    let abi = Contract::load(StringReader::new(CPP_ABI))?;

    log_operation("deploy cpp");
    let cpp = evm.deploy(EthDeployment {
        caller: DEPLOYER,
        data: CPP_BYTECODE.into(),
        ..Default::default()
    })?;

    log_operation("init token");
    evm.transact(EthTransaction {
        caller: DEPLOYER,
        contract: cpp.clone(),
        data: data!(abi.initialize(brlc)),
        ..Default::default()
    })?;

    Ok((cpp, abi))
}

fn deploy_pix(evm: &EthExecutor, brlc: Address) -> eyre::Result<(Address, Contract)> {
    let abi = Contract::load(StringReader::new(PIX_ABI))?;

    log_operation("deploy pix");
    let pix = evm.deploy(EthDeployment {
        caller: DEPLOYER,
        data: PIX_BYTECODE.into(),
        ..Default::default()
    })?;

    log_operation("init token");
    evm.transact(EthTransaction {
        caller: DEPLOYER,
        contract: pix.clone(),
        data: data!(abi.initialize(brlc)),
        ..Default::default()
    })?;

    log_operation("grant role");
    evm.transact(EthTransaction {
        caller: DEPLOYER,
        contract: pix.clone(),
        data: data!(abi.grantRole(
            Token::FixedBytes(hex!("ff3317e1e0e8c784290b957ee9b65ea8fe96420c4819db463283b95ecbe4a3fe").into()),
            MINTER
        )),
        ..Default::default()
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
