fn u256_to_bytes(u: ethereum_types::U256) -> Vec<u8> {
    let mut bytes = [0u8; 32];
    u.to_big_endian(&mut bytes);
    bytes.to_vec()
}
