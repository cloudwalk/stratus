use tiny_keccak::Hasher;
use tiny_keccak::Keccak;

pub fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut keccak = Keccak::v256();
    let mut keccak_output: [u8; 32] = [0; 32];
    keccak.update(data);
    keccak.finalize(&mut keccak_output);

    keccak_output
}
