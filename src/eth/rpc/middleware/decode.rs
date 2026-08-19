use alloy_dyn_abi::DynSolValue;
use alloy_dyn_abi::JsonAbiExt;
use alloy_json_abi::Function;

use crate::eth::codegen::SIGNATURES_4_BYTES;
use crate::eth::types::DecodeInputError;

/// Decodes the input arguments of a transaction.
pub fn decode_input_arguments(input: impl AsRef<[u8]>) -> Result<String, DecodeInputError> {
    let input = input.as_ref();
    let Some((selector, param_data)) = input.split_at_checked(4) else {
        return Err(DecodeInputError::InputTooShort {
            message: format!("expected at least 4 bytes for function selector, got {} bytes", input.len()),
        });
    };

    let Some(signature) = SIGNATURES_4_BYTES.get(selector) else {
        return Err(DecodeInputError::FunctionUnknown {
            message: format!("selector 0x{} not found in signature mapping", const_hex::encode(selector)),
        });
    };

    let func = Function::parse(signature).map_err(|e| DecodeInputError::InvalidAbi {
        message: format!("invalid signature format: {signature} ({e})"),
    })?;
    let tokens = func.abi_decode_input(param_data)?;
    Ok(format_tokens_human_readable(&tokens))
}

// Formats decoded tokens into a human-readable string.
fn format_tokens_human_readable(tokens: &[DynSolValue]) -> String {
    let items: Vec<String> = tokens.iter().map(format_token).collect();
    format!("({})", items.join(", "))
}

fn format_token(token: &DynSolValue) -> String {
    match token {
        DynSolValue::Address(addr) => format!("0x{addr:x}"),
        DynSolValue::Uint(val, _) => format!("{val}"),
        DynSolValue::Int(val, _) => format!("{val}"),
        DynSolValue::Bool(val) => val.to_string(),
        DynSolValue::String(val) => format!("\"{val}\""),
        DynSolValue::Bytes(val) => format!("0x{}", const_hex::encode(val)),
        DynSolValue::FixedBytes(val, size) => format!("0x{}", const_hex::encode(&val.as_slice()[..*size])),
        DynSolValue::Array(arr) | DynSolValue::FixedArray(arr) => {
            let items: Vec<String> = arr.iter().map(format_token).collect();
            format!("[{}]", items.join(", "))
        }
        DynSolValue::Tuple(tuple) => {
            let items: Vec<String> = tuple.iter().map(format_token).collect();
            format!("({})", items.join(", "))
        }
        DynSolValue::Function(f) => format!("0x{}", const_hex::encode(f.as_slice())),
    }
}

#[cfg(test)]
mod tests {
    use alloy_dyn_abi::DynSolValue;
    use alloy_dyn_abi::JsonAbiExt;
    use alloy_json_abi::Function;
    use alloy_primitives::Address;
    use alloy_primitives::B256;
    use alloy_primitives::I256;
    use alloy_primitives::U256;
    use hex_literal::hex;

    use super::*;

    #[test]
    fn test_parse_transfer_transaction_input() {
        // Test transfer(address,uint256)
        let func = Function::parse("transfer(address,uint256)").unwrap();
        let tx_transfer = func
            .abi_encode_input(&[
                DynSolValue::Address(Address::from(hex!("1234567890123456789012345678901234567890"))),
                DynSolValue::Uint(U256::from(1000000000000000000u64), 256),
            ])
            .unwrap();
        let result = decode_input_arguments(&tx_transfer).unwrap();

        assert_eq!(result, "(0x1234567890123456789012345678901234567890, 1000000000000000000)");
    }

    #[test]
    fn test_decode_input_arguments_with_dynamic_array() {
        let func = Function::parse("grantRoleBatch(bytes32,address[])").unwrap();
        let input = func
            .abi_encode_input(&[
                DynSolValue::FixedBytes(B256::repeat_byte(0xab), 32),
                DynSolValue::Array(vec![
                    DynSolValue::Address(Address::repeat_byte(0x11)),
                    DynSolValue::Address(Address::repeat_byte(0x22)),
                ]),
            ])
            .unwrap();

        let result = decode_input_arguments(&input).unwrap();

        assert_eq!(
            result,
            "(0xababababababababababababababababababababababababababababababab, \
             [0x1111111111111111111111111111111111111111, \
             0x2222222222222222222222222222222222222222])"
        );
    }

    #[test]
    fn test_no_parameter_input() {
        // Test underlying()
        let tx_no_parameter = Vec::from(hex!("18160ddd"));
        let result = decode_input_arguments(&tx_no_parameter).unwrap();

        assert_eq!(result, "()");
    }

    #[test]
    fn test_complex_input() {
        // Test test(uint256,(string,bool,(int256,uint256[]))[])
        let signature = "test(uint256,(string,bool,(int256,uint256[]))[])";
        let func = Function::parse(signature).unwrap();
        let param_data = func
            .abi_encode_input_raw(&[
                DynSolValue::Uint(U256::from(1000000000000000000u64), 256),
                DynSolValue::Array(vec![DynSolValue::Tuple(vec![
                    DynSolValue::String("test".to_string()),
                    DynSolValue::Bool(true),
                    DynSolValue::Tuple(vec![
                        DynSolValue::Int(I256::unchecked_from(200000000000000000i128), 256),
                        DynSolValue::Array(vec![DynSolValue::Uint(U256::from(3000000000000000000u64), 256)]),
                    ]),
                ])]),
            ])
            .unwrap();

        let tokens = func.abi_decode_input(&param_data).unwrap();
        let result = format_tokens_human_readable(&tokens);
        assert_eq!(
            result,
            r#"(1000000000000000000, [("test", true, (200000000000000000, [3000000000000000000]))])"#
        );
    }

    #[test]
    fn test_invalid_input() {
        let invalid_input = Vec::from(hex!("a9059cbb"));
        let result = decode_input_arguments(&invalid_input);
        assert!(matches!(result, Err(DecodeInputError::InvalidInput { source: _ })));
    }

    #[test]
    fn test_invalid_length() {
        let invalid_input = Vec::from(hex!("a9059c"));
        let result = decode_input_arguments(&invalid_input);
        assert!(matches!(result, Err(DecodeInputError::InputTooShort { message: _ })));
    }

    #[test]
    fn test_invalid_signature() {
        let invalid_input = Vec::from(hex!("42000042"));
        let result = decode_input_arguments(&invalid_input);
        assert!(matches!(result, Err(DecodeInputError::FunctionUnknown { message: _ })));
    }
}
