//! Bytecode should be an struct replacing Option<Bytes> inside account, but changing it now will break a lot of thing,
//! so we will keep it as is for now.

use std::collections::HashSet;

use evm_disassembler::disassemble_bytes;
use evm_disassembler::Opcode;
use evm_disassembler::Opcode::MSTORE;
use evm_disassembler::Opcode::PUSH0;
use evm_disassembler::Opcode::PUSH1;
use evm_disassembler::Opcode::PUSH10;
use evm_disassembler::Opcode::PUSH11;
use evm_disassembler::Opcode::PUSH12;
use evm_disassembler::Opcode::PUSH13;
use evm_disassembler::Opcode::PUSH14;
use evm_disassembler::Opcode::PUSH15;
use evm_disassembler::Opcode::PUSH16;
use evm_disassembler::Opcode::PUSH17;
use evm_disassembler::Opcode::PUSH18;
use evm_disassembler::Opcode::PUSH19;
use evm_disassembler::Opcode::PUSH2;
use evm_disassembler::Opcode::PUSH20;
use evm_disassembler::Opcode::PUSH21;
use evm_disassembler::Opcode::PUSH22;
use evm_disassembler::Opcode::PUSH23;
use evm_disassembler::Opcode::PUSH24;
use evm_disassembler::Opcode::PUSH25;
use evm_disassembler::Opcode::PUSH26;
use evm_disassembler::Opcode::PUSH27;
use evm_disassembler::Opcode::PUSH28;
use evm_disassembler::Opcode::PUSH29;
use evm_disassembler::Opcode::PUSH3;
use evm_disassembler::Opcode::PUSH30;
use evm_disassembler::Opcode::PUSH31;
use evm_disassembler::Opcode::PUSH32;
use evm_disassembler::Opcode::PUSH4;
use evm_disassembler::Opcode::PUSH5;
use evm_disassembler::Opcode::PUSH6;
use evm_disassembler::Opcode::PUSH7;
use evm_disassembler::Opcode::PUSH8;
use evm_disassembler::Opcode::PUSH9;
use evm_disassembler::Opcode::SHA3;
use evm_disassembler::Opcode::SLOAD;
use evm_disassembler::Operation;
use itertools::Itertools;

use crate::eth::primitives::Bytes;
use crate::eth::primitives::SlotAccess;
use crate::eth::primitives::SlotIndex;

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
    for w in opcodes.windows(7) {
        let (op1, op2, op3, op4, _, op6, op7) = (&w[0], &w[1], &w[2], &w[3], &w[4], &w[5], &w[6]);

        // direct SLOAD
        // PUSH (index) -> SLOAD
        if is_push(op1) && op2.opcode == SLOAD {
            slots.insert((SlotIndex::from(op1.input.clone()), SlotAccess::Direct));
            continue;
        }

        // mapping SLOAD
        // PUSH (index) -> MSTORE -> PUSH1 0x20 -> PUSH1 0x40 -> ? -> SHA3 -> SLOAD
        if is_push(op1)
            && (is_push(op2) && op2.input[0] == 32)
            && op3.opcode == MSTORE
            && (is_push(op4) && op4.input[0] == 64)
            && op6.opcode == SHA3
            && op7.opcode == SLOAD
        {
            slots.insert((SlotIndex::from(op1.input.clone()), SlotAccess::Mapping));
            continue;
        }
    }

    for slot in slots.iter().sorted() {
        println!("{:<10} = {:?}", format!("{:?}", slot.1), slot.0);
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
    use crate::eth::primitives::Bytes;

    const BYTECODE: &str = include_str!("../../../tests/fixtures/bytecode_brlc_token.bin");

    #[test]
    fn test_parse_bytecode_slots() {
        let bytecode = Bytes(const_hex::decode(BYTECODE).unwrap());
        super::parse_bytecode_slots(bytecode);
    }
}
