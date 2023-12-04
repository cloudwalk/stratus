use hex_literal::hex;

// -----------------------------------------------------------------------------
// Contracts
// -----------------------------------------------------------------------------
pub const CONTRACT_BRLC: [u8; 20] = hex!("c6d1efd908ef6b69da0749600f553923c465c812");

const BYTECODE_BRLC_FULL: &str = include_str!("../../assets/BRLCToken.bin");
const BYTECODE_BRLC_RUNTIME: &str = include_str!("../../assets/BRLCToken.bin-runtime");
const BYTECODE_TEST_FULL: &str = include_str!("../../assets/simple.bin");
const BYTECIDE_TEST_RUNTIME: &str = include_str!("../../assets/simple.bin-runtime");

pub fn bytecode_brlc() -> Vec<u8> {
    hex::decode(BYTECODE_BRLC_FULL).unwrap()
}

pub fn bytecode_brlc_runtime() -> Vec<u8> {
    hex::decode(BYTECODE_BRLC_RUNTIME).unwrap()
}

pub fn bytecode_test() -> Vec<u8> {
    hex::decode(BYTECODE_TEST_FULL).unwrap()
}

pub fn bytecode_test_runtime() -> Vec<u8> {
    hex::decode(BYTECIDE_TEST_RUNTIME).unwrap()
}

// -----------------------------------------------------------------------------
// ERC20 contract operations
// -----------------------------------------------------------------------------
pub const TRANSFER_ORIGIN: [u8; 20] = hex!("2b2d06a43b4701ae764366eed78dd30613c86eee");
pub const TRANSFER_DESTINATION: [u8; 20] = hex!("1234567890123456789012345678901234567890");
pub const TRASNFER_DATA: [u8; 68] =
    hex!("a9059cbb000000000000000000000000cbd099b0173ef70f17f84846f66fa0fbb4bdeeee0000000000000000000000000000000000000000000000000000000000000000");

// -----------------------------------------------------------------------------
// Test contract operations
// -----------------------------------------------------------------------------
pub const SIMPLE_DATA_INC: [u8; 4] = hex!("371303c0");
pub const SIMPLE_DATA_READ: [u8; 4] = hex!("57de26a4");
