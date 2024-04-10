//! Bytecode should be an struct replacing Option<Bytes> inside account, but changing it now will break a lot of thing,
//! so we will keep it as is for now.

use std::collections::HashSet;

use evm_disassembler::disassemble_bytes;
use evm_disassembler::Opcode;
use evm_disassembler::Opcode::*;
use evm_disassembler::Operation;

use crate::eth::primitives::Bytes;
use crate::eth::primitives::SlotAccess;
use crate::eth::primitives::SlotIndex;

/// Parse all accessed storage slots from bytecode.
///
/// TODO: Decide about return type, if we should use a tuple or some other struct.
pub fn parse_bytecode_slots(bytecode: Bytes) -> HashSet<(SlotIndex, SlotAccess)> {
    // parse opcodes
    let opcodes = match disassemble_bytes(bytecode.0) {
        Ok(opcodes) => opcodes,
        Err(e) => {
            tracing::error!(reason = ?e, "failed to parse opcodes. not a contract?");
            return HashSet::new();
        }
    };

    // detect slots
    let mut slots = HashSet::with_capacity(16);
    for w in opcodes.windows(4) {
        let (op1, op2, op3, op4) = (&w[0], &w[1], &w[2], &w[3]);

        // Direct: PUSH (index) -> SLOAD
        if is_push(op1) && op2.opcode == SLOAD {
            slots.insert((op1.input.clone().into(), SlotAccess::Direct));
            continue;
        }

        // Direct: PUSH1 -> DUP1 -> SLOAD
        if op1.opcode == PUSH1 && op2.opcode == DUP1 && op3.opcode == SLOAD {
            slots.insert((op1.input.clone().into(), SlotAccess::Direct));
            continue;
        }

        // Direct: PUSH1 -> PUSH1 -> SWAP1 -> SLOAD
        if op1.opcode == PUSH1 && op2.opcode == PUSH1 && op3.opcode == SWAP1 && op4.opcode == SLOAD {
            slots.insert((op1.input.clone().into(), SlotAccess::Direct));
            continue;
        }

        // Mapping: PUSH (index) -> PUSH1 0x20 -> MSTORE -> PUSH1 0x40
        if is_push(op1) && (is_push(op2) && op2.input[0] == 0x20) && op3.opcode == MSTORE && (is_push(op4) && op4.input[0] == 0x40) {
            slots.insert((op1.input.clone().into(), SlotAccess::Mapping));
            continue;
        }

        // TODO: Array?
    }

    slots
}

// -----------------------------------------------------------------------------
// Checks
// -----------------------------------------------------------------------------
const OPCODES_PUSH: [Opcode; 33] = [
    PUSH0, PUSH1, PUSH2, PUSH3, PUSH4, PUSH5, PUSH6, PUSH7, PUSH8, PUSH9, PUSH10, PUSH11, PUSH12, PUSH13, PUSH14, PUSH15, PUSH16, PUSH17, PUSH18, PUSH19,
    PUSH20, PUSH21, PUSH22, PUSH23, PUSH24, PUSH25, PUSH26, PUSH27, PUSH28, PUSH29, PUSH30, PUSH31, PUSH32,
];
fn is_push(operation: &Operation) -> bool {
    OPCODES_PUSH.contains(&operation.opcode)
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use itertools::Itertools;

    use crate::eth::primitives::Bytes;
    use crate::eth::primitives::SlotAccess;
    use crate::eth::primitives::SlotIndex;

    const BYTECODE_BRLC_TOKEN: &str = include_str!("../../../tests/fixtures/bytecodes/BRLCToken.bin");
    const BYTECODE_CPP_V1: &str = include_str!("../../../tests/fixtures/bytecodes/CardPaymentProcessor.bin");
    const BYTECODE_PIX: &str = include_str!("../../../tests/fixtures/bytecodes/PixCashier.bin");

    #[test]
    // TODO: add assertions based on storage layout file generated from source code.
    fn test_parse_bytecode_slots() {
        // brlc token
        let brlc_token = Bytes(const_hex::decode(BYTECODE_BRLC_TOKEN).unwrap());
        print_slots("BRLC", super::parse_bytecode_slots(brlc_token));

        // // cpp
        let cpp_v1 = Bytes(const_hex::decode(BYTECODE_CPP_V1).unwrap());
        print_slots("CPP", super::parse_bytecode_slots(cpp_v1));

        // pix
        let pix = Bytes(const_hex::decode(BYTECODE_PIX).unwrap());
        print_slots("Pix", super::parse_bytecode_slots(pix));
    }

    fn print_slots(name: &'static str, slots: HashSet<(SlotIndex, SlotAccess)>) {
        println!("\n{}\n--------------------", name);
        for slot in slots.iter().sorted() {
            println!("{:<7} = {:?}", format!("{:?}", slot.1), slot.0);
        }
    }
}
