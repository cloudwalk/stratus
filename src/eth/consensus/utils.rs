use anyhow::{Result, anyhow};
use crate::eth::primitives::U256;

pub fn u256_to_bytes(u: U256) -> Vec<u8> {
    let mut bytes = [0u8; 32];
    u.to_big_endian(&mut bytes);
    bytes.to_vec()
}

pub fn bytes_to_u256(bytes: &[u8]) -> Result<U256> {
    if bytes.len() > 32 {
        Err(anyhow!("Byte array too long to convert to U256"))
    } else {
        Ok(U256::from_big_endian(bytes))
    }
}
