#[cfg(test)]
mod tests {
    use super::*;
    use ethereum_types::H256;
    use crate::eth::primitives::Hash;

    #[test]
    fn test_hash_new() {
        let bytes = [0u8; 32];
        let hash = Hash::new(bytes);
        assert_eq!(hash.0, H256(bytes));
    }

    #[test]
    fn test_hash_zero() {
        assert_eq!(Hash::ZERO.0, H256::zero());
    }

    #[test]
    fn test_hash_display() {
        let bytes = [0xffu8; 32];
        let hash = Hash::new(bytes);
        assert_eq!(hash.to_string(), "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    }

    #[test]
    fn test_hash_as_ref() {
        let bytes = [1u8; 32];
        let hash = Hash::new(bytes);
        let slice: &[u8] = hash.as_ref();
        assert_eq!(slice, &bytes);
    }

    #[test]
    fn test_hash_from_str_valid() {
        let hash_str = "0x0000000000000000000000000000000000000000000000000000000000000000";
        let hash = Hash::from_str(hash_str).unwrap();
        assert_eq!(hash.0, H256::zero());
    }

    #[test]
    fn test_hash_from_str_invalid() {
        let result = Hash::from_str("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_hash_conversions() {
        let bytes = [2u8; 32];
        let h256 = H256(bytes);
        let hash: Hash = h256.into();
        let back: H256 = hash.into();
        assert_eq!(h256, back);
    }
}
