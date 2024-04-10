//! Bytecode should be an struct replacing Option<Bytes> inside account, but changing it now will break a lot of thing,
//! so we will keep it as is for now.

use std::collections::HashSet;

use evm_disassembler::disassemble_bytes;
use evm_disassembler::Opcode::PUSH1;
use evm_disassembler::Opcode::PUSH2;
use evm_disassembler::Opcode::PUSH0;
use evm_disassembler::Opcode::SLOAD;
use itertools::Itertools;

use crate::eth::primitives::Bytes;
use crate::eth::primitives::SlotIndex;

pub fn parse_bytecode_slots(bytecode: Bytes) -> HashSet<SlotIndex> {
    // parse opcodes
    let opcodes = match disassemble_bytes(bytecode.0) {
        Ok(opcodes) => opcodes,
        Err(e) => {
            tracing::error!(reason = ?e, "failed to parse opcodes. not a contract?");
            return HashSet::new()
        }
    };

    // detect slots
    let mut slots = HashSet::with_capacity(16);
    for window in opcodes.windows(2) {
        let (op1, op2) = (&window[0], &window[1]);

        // (PUSH0 | PUSH1 | PUSH2) -> SLOAD
        if [PUSH0, PUSH1, PUSH2].contains(&op1.opcode) && op2.opcode == SLOAD {
            if op1.opcode == PUSH0 {
                slots.insert(SlotIndex::ZERO);
            } else {
                slots.insert(SlotIndex::from(op1.input.clone()));
            }
            continue;
        }
    }

    println!("{:?}", slots.iter().sorted().collect_vec());
    slots
}

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
