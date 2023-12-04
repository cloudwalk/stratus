mod common;

use binary_macros::base16;
use ethabi::Contract;
use ethabi::Token;
use hex_literal::hex;
use ledger::evm::entities::Address;
use ledger::evm::entities::Amount;
use ledger::evm::revm::Revm;
use ledger::evm::Evm;
use primitive_types::H160;
use stringreader::StringReader;

static BRLC_ABI: &str = include_str!("contracts/BRLCToken.abi");
static BRLC_BYTECODE: &[u8] = base16!("file:tests/contracts/BRLCToken.bin");

static CPP_ABI: &str = include_str!("contracts/CardPaymentProcessor.abi");
static CPP_BYTECODE: &[u8] = base16!("file:tests/contracts/CardPaymentProcessor.bin");

static PIX_ABI: &str = include_str!("contracts/PixCashier.abi");
static PIX_BYTECODE: &[u8] = base16!("file:tests/contracts/PixCashier.bin");

const DEPLOYER: Address = Address(H160(hex!("1111111111111111111111111111111111111111")));
const MINTER: Address = Address(H160(hex!("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee")));
const TEST1: Address = Address(H160(hex!("1111222233334444555566667777888899990000")));
const TEST2: Address = Address(H160(hex!("0000999988887777666655554444333322221111")));

use serial_test::serial;

#[test]
#[serial]
fn evm_brlc() -> eyre::Result<()> {
    // deploy
    let (mut evm, _) = common::init_testenv();
    let (brlc, abi) = deploy_brlc(&mut evm)?;

    // execute
    log_operation("mint");
    evm.from(&MINTER).to(&brlc).write(data!(abi.mint(TEST1, Amount::from(u16::MAX as usize))))?;

    log_operation("balance");
    evm.to(&brlc).read(data!(abi.balanceOf(TEST1)))?;

    log_operation("transfer");
    evm.from(&TEST1).to(&brlc).write(data!(abi.transfer(TEST2, Amount::from(u8::MAX as usize))))?;

    log_operation("balance");
    evm.to(&brlc).read(data!(abi.balanceOf(TEST1)))?;

    log_operation("transfer too large");
    evm.from(&TEST1).to(&brlc).write(data!(abi.transfer(TEST2, Amount::from(u32::MAX as usize))))?;

    log_operation("balance");
    evm.to(&brlc).read(data!(abi.balanceOf(TEST1)))?;

    Ok(())
}

#[test]
#[serial]
fn evm_cpp() -> eyre::Result<()> {
    // deploy
    let (mut evm, _) = common::init_testenv();
    let (brlc, _) = deploy_brlc(&mut evm)?;
    let (cpp, abi) = deploy_cpp(&mut evm, &brlc)?;

    // execute
    log_operation("make payment");
    evm.from(&TEST1).to(&cpp).write(data!(abi.makePayment(
        Amount::from(0),
        Amount::from(0),
        Token::FixedBytes(vec![1u8; 16]),
        Token::FixedBytes(vec![1u8; 16])
    )))?;

    log_operation("get payment");
    evm.to(&cpp).read(data!(abi.paymentFor(Token::FixedBytes(vec![1u8; 16]))))?;

    Ok(())
}

#[test]
#[serial]
fn evm_pix() -> eyre::Result<()> {
    // deploy
    let (mut evm, _) = common::init_testenv();
    let (brlc, _) = deploy_brlc(&mut evm)?;
    let (pix, abi) = deploy_pix(&mut evm, &brlc)?;

    // execute
    log_operation("cash-in");
    evm.from(&MINTER)
        .to(&pix)
        .write(data!(abi.cashIn(TEST1, Amount::from(1), Token::FixedBytes(vec![1u8; 32]))))?;

    Ok(())
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn deploy_brlc(evm: &mut Revm) -> eyre::Result<(Address, Contract)> {
    let abi = Contract::load(StringReader::new(BRLC_ABI))?;

    log_operation("deploy brlc");
    let brlc = evm.from(&DEPLOYER).deploy(BRLC_BYTECODE)?;

    log_operation("brlc owner");
    evm.to(&brlc).read(data!(abi.owner()))?;

    log_operation("configure master minter");
    evm.from(&DEPLOYER).to(&brlc).write(data!(abi.updateMasterMinter(MINTER)))?;

    log_operation("configure minter");
    evm.from(&DEPLOYER)
        .to(&brlc)
        .write(data!(abi.configureMinter(MINTER, Amount::from(u64::MAX as usize))))?;

    Ok((brlc, abi))
}

fn deploy_cpp(evm: &mut Revm, brlc: &Address) -> eyre::Result<(Address, Contract)> {
    let abi = Contract::load(StringReader::new(CPP_ABI))?;

    log_operation("deploy cpp");
    let cpp = evm.from(&DEPLOYER).deploy(CPP_BYTECODE)?;

    log_operation("init token");
    evm.from(&DEPLOYER).to(&cpp).write(data!(abi.initialize(brlc.clone())))?;

    Ok((cpp, abi))
}

fn deploy_pix(evm: &mut Revm, brlc: &Address) -> eyre::Result<(Address, Contract)> {
    let abi = Contract::load(StringReader::new(PIX_ABI))?;

    log_operation("deploy pix");
    let pix = evm.from(&DEPLOYER).deploy(PIX_BYTECODE)?;

    log_operation("init token");
    evm.from(&DEPLOYER).to(&pix).write(data!(abi.initialize(brlc.clone())))?;

    log_operation("grant role");
    evm.from(&DEPLOYER).to(&pix).write(data!(abi.grantRole(
        Token::FixedBytes(hex!("ff3317e1e0e8c784290b957ee9b65ea8fe96420c4819db463283b95ecbe4a3fe").into()),
        MINTER
    )))?;

    Ok((pix, abi))
}

fn log_operation(s: &str) {
    println!();
    println!("----------------------------------------");
    println!("{}", s.to_uppercase());
    println!("----------------------------------------");
    println!();
}
