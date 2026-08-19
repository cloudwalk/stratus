use alloy_dyn_abi::DynSolType;
use alloy_dyn_abi::DynSolValue;

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

    let param_types = parse_to_param_types(signature)?;
    let DynSolValue::Tuple(tokens) = DynSolType::Tuple(param_types).abi_decode_params(param_data)? else {
        return Err(DecodeInputError::InvalidAbi {
            message: "decoded parameters did not form a tuple".to_string(),
        });
    };
    Ok(format_tokens_human_readable(&tokens))
}

/// Parses a Solidity function signature string into parameter types.
/// Example: "transfer(address,uint256)" -> [DynSolType::Address, DynSolType::Uint(256)]
fn parse_to_param_types(signature: &str) -> Result<Vec<DynSolType>, DecodeInputError> {
    let start = signature.find('(').ok_or_else(|| DecodeInputError::InvalidAbi {
        message: format!("invalid signature format: {signature} (missing opening parenthesis)"),
    })?;
    let end = signature.rfind(')').ok_or_else(|| DecodeInputError::InvalidAbi {
        message: format!("invalid signature format: {signature} (missing closing parenthesis)"),
    })?;
    let tuple_str = &signature[start..=end];
    let ty = DynSolType::parse(tuple_str).map_err(|e| DecodeInputError::InvalidAbi {
        message: format!("invalid signature format: {signature} ({e})"),
    })?;
    match ty {
        DynSolType::Tuple(inner) => Ok(inner),
        other => Err(DecodeInputError::InvalidAbi {
            message: format!("invalid signature format: {signature} (expected parameter tuple, got {other})"),
        }),
    }
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
    use alloy_dyn_abi::DynSolType;
    use alloy_dyn_abi::DynSolValue;
    use alloy_primitives::Address;
    use alloy_primitives::I256;
    use alloy_primitives::U256;
    use hex_literal::hex;

    use super::*;

    #[test]
    fn test_parse_transfer_transaction_input() {
        // Test transfer(address,uint256)
        let mut tx_transfer = Vec::from(hex!("a9059cbb"));
        tx_transfer.extend_from_slice(
            &DynSolValue::Tuple(vec![
                DynSolValue::Address(Address::from(hex!("1234567890123456789012345678901234567890"))),
                DynSolValue::Uint(U256::from(1000000000000000000u64), 256),
            ])
            .abi_encode_params(),
        );
        let result = decode_input_arguments(&tx_transfer).unwrap();

        assert_eq!(result, "(0x1234567890123456789012345678901234567890, 1000000000000000000)");
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
        let param_types = parse_to_param_types(signature).unwrap();
        assert_eq!(
            param_types,
            vec![
                DynSolType::Uint(256),
                DynSolType::Array(Box::new(DynSolType::Tuple(vec![
                    DynSolType::String,
                    DynSolType::Bool,
                    DynSolType::Tuple(vec![DynSolType::Int(256), DynSolType::Array(Box::new(DynSolType::Uint(256)))]),
                ])))
            ]
        );
        let param_data = DynSolValue::Tuple(vec![
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
        .abi_encode_params();

        let value = DynSolType::Tuple(param_types)
            .abi_decode_params(&param_data)
            .expect("failed to decode parameters");
        let DynSolValue::Tuple(tokens) = value else {
            panic!("expected decoded value to be a tuple");
        };
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
