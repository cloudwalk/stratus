use ethabi::ParamType;
use ethabi::Token;

use crate::eth::codegen::SIGNATURES_4_BYTES;
use crate::eth::primitives::DecodeInputError;

/// Decodes the input arguments of a transaction.
pub fn decode_input_arguments(input: impl AsRef<[u8]>) -> Result<String, DecodeInputError> {
    let Some(selector) = input.as_ref().get(..4) else {
        return Err(DecodeInputError::InputTooShort {
            message: format!("expected at least 4 bytes for function selector, got {} bytes", input.as_ref().len()),
        });
    };
    let Some(signature) = SIGNATURES_4_BYTES.get(selector) else {
        return Err(DecodeInputError::FunctionUnknown {
            message: format!("selector 0x{} not found in signature mapping", const_hex::encode(selector)),
        });
    };
    let param_types = parse_to_param_types(signature)?;
    let param_data = input.as_ref().get(4..).ok_or(DecodeInputError::InputTooShort {
        message: "input data too short for parameters after function selector".to_string(),
    })?;
    let tokens = ethabi::decode(&param_types, param_data)?;
    Ok(format_tokens_human_readable(&tokens))
}

/// Parses a Solidity function signature string into parameter types.
/// Example: "transfer(address,uint256)" -> [ParamType::Address, ParamType::Uint(256)]
fn parse_to_param_types(signature: &str) -> Result<Vec<ParamType>, DecodeInputError> {
    let start = signature.find('(').ok_or_else(|| DecodeInputError::InvalidAbi {
        message: format!("invalid signature format: {signature} (missing opening parenthesis)"),
    })?;
    let end = signature.rfind(')').ok_or_else(|| DecodeInputError::InvalidAbi {
        message: format!("invalid signature format: {signature} (missing closing parenthesis)"),
    })?;
    let params_str = &signature[start + 1..end];
    if params_str.is_empty() {
        return Ok(Vec::new());
    }
    tokenize_parameters(params_str)
}

/// Tokenizes parameter string while respecting nested parentheses for tuples.
/// Example: "address,(uint32,uint32,uint64),bool" -> ["address", "(uint32,uint32,uint64)", "bool"]
fn tokenize_parameters(params_str: &str) -> Result<Vec<ParamType>, DecodeInputError> {
    let mut tokens = Vec::new();
    let mut current_token = String::new();
    let mut paren_depth = 0;
    let mut bracket_open = false;

    for ch in params_str.chars() {
        match ch {
            '(' => {
                paren_depth += 1;
                current_token.push(ch);
            }
            ')' => {
                paren_depth -= 1;
                current_token.push(ch);
                if paren_depth < 0 {
                    return Err(DecodeInputError::InvalidAbi {
                        message: "unmatched closing parenthesis".to_string(),
                    });
                }
            }
            '[' => {
                if bracket_open {
                    return Err(DecodeInputError::InvalidAbi {
                        message: "nested brackets are not allowed".to_string(),
                    });
                }
                bracket_open = true;
                current_token.push(ch);
            }
            ']' => {
                if !bracket_open {
                    return Err(DecodeInputError::InvalidAbi {
                        message: "unmatched closing bracket".to_string(),
                    });
                }
                bracket_open = false;
                current_token.push(ch);
            }
            ',' => {
                if paren_depth == 0 && !bracket_open {
                    // We're at the top level, this comma separates parameters
                    tokens.push(parse_solidity_type(current_token.trim())?);
                    current_token.clear();
                } else {
                    // We're inside parentheses or brackets, this comma is part of the current token
                    current_token.push(ch);
                }
            }
            _ => {
                current_token.push(ch);
            }
        }
    }

    // Add the last token
    if !current_token.trim().is_empty() {
        tokens.push(parse_solidity_type(current_token.trim())?);
    }

    // Check for unmatched parentheses/brackets
    if paren_depth != 0 {
        return Err(DecodeInputError::InvalidAbi {
            message: "unmatched parentheses".to_string(),
        });
    }
    if bracket_open {
        return Err(DecodeInputError::InvalidAbi {
            message: "unmatched brackets".to_string(),
        });
    }

    Ok(tokens)
}

fn parse_solidity_type(type_str: impl AsRef<str>) -> Result<ParamType, DecodeInputError> {
    let type_str = type_str.as_ref();
    match type_str {
        s if s.ends_with("[]") => {
            let inner_type = parse_solidity_type(&s[..s.len() - 2])?;
            Ok(ParamType::Array(Box::new(inner_type)))
        }
        s if s.contains('[') && s.ends_with(']') => {
            let bracket_pos = s.find('[').unwrap();
            let inner_type = parse_solidity_type(&s[..bracket_pos])?;
            let size_str = &s[bracket_pos + 1..s.len() - 1];
            let size = size_str.parse::<usize>().map_err(|_| DecodeInputError::InvalidAbi {
                message: format!("invalid array size in type: {s}"),
            })?;
            Ok(ParamType::FixedArray(Box::new(inner_type), size))
        }
        s if s.starts_with('(') && s.ends_with(')') => {
            // Parse tuple type: (uint32,uint32,uint64) -> ParamType::Tuple
            let inner_str = &s[1..s.len() - 1]; // Remove outer parentheses

            if inner_str.is_empty() {
                return Ok(ParamType::Tuple(Vec::new()));
            }

            Ok(ParamType::Tuple(tokenize_parameters(inner_str)?))
        }
        "address" => Ok(ParamType::Address),
        "bool" => Ok(ParamType::Bool),
        "string" => Ok(ParamType::String),
        "bytes" => Ok(ParamType::Bytes),
        s if s.starts_with("uint") => {
            let size = if s == "uint" {
                256
            } else {
                s[4..].parse::<usize>().map_err(|_| DecodeInputError::InvalidAbi {
                    message: format!("invalid uint size in type: {s}"),
                })?
            };
            Ok(ParamType::Uint(size))
        }
        s if s.starts_with("int") => {
            let size = if s == "int" {
                256
            } else {
                s[3..].parse::<usize>().map_err(|_| DecodeInputError::InvalidAbi {
                    message: format!("invalid int size in type: {s}"),
                })?
            };
            Ok(ParamType::Int(size))
        }
        s if s.starts_with("bytes") && s.len() > 5 => {
            let size = s[5..].parse::<usize>().map_err(|_| DecodeInputError::InvalidAbi {
                message: format!("invalid bytes size in type: {s}"),
            })?;
            Ok(ParamType::FixedBytes(size))
        }
        _ => Err(DecodeInputError::InvalidAbi {
            message: format!("unsupported Solidity type: {type_str}"),
        }),
    }
}

// Formats decoded tokens into a human-readable string.
fn format_tokens_human_readable(tokens: &[Token]) -> String {
    let items: Vec<String> = tokens.iter().map(format_token).collect();
    format!("({})", items.join(", "))
}

fn format_token(token: &Token) -> String {
    match token {
        Token::Address(addr) => format!("0x{addr:x}"),
        Token::Uint(val) | Token::Int(val) => format!("{val}"),
        Token::Bool(val) => val.to_string(),
        Token::String(val) => format!("\"{val}\""),
        Token::Bytes(val) => format!("0x{}", const_hex::encode(val)),
        Token::FixedBytes(val) => format!("0x{}", const_hex::encode(val)),
        Token::Array(arr) | Token::FixedArray(arr) => {
            let items: Vec<String> = arr.iter().map(format_token).collect();
            format!("[{}]", items.join(", "))
        }
        Token::Tuple(tuple) => {
            let items: Vec<String> = tuple.iter().map(format_token).collect();
            format!("({})", items.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use ethabi::Address;
    use ethabi::ParamType;
    use hex_literal::hex;

    use super::*;

    #[test]
    fn test_parse_transfer_transaction_input() {
        // Test transfer(address,uint256)
        let mut tx_transfer = Vec::from(hex!("a9059cbb"));
        tx_transfer.extend_from_slice(&ethabi::encode(&[
            Token::Address(Address::from(hex!("1234567890123456789012345678901234567890"))),
            Token::Uint(1000000000000000000u64.into()),
        ]));
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
                ParamType::Uint(256),
                ParamType::Array(Box::new(ParamType::Tuple(vec![
                    ParamType::String,
                    ParamType::Bool,
                    ParamType::Tuple(vec![ParamType::Int(256), ParamType::Array(Box::new(ParamType::Uint(256)))]),
                ])))
            ]
        );
        let param_data = ethabi::encode(&[
            Token::Uint(1000000000000000000u64.into()),
            Token::Array(vec![Token::Tuple(vec![
                Token::String("test".to_string()),
                Token::Bool(true),
                Token::Tuple(vec![
                    Token::Int(200000000000000000i128.into()),
                    Token::Array(vec![Token::Uint(3000000000000000000u64.into())]),
                ]),
            ])]),
        ]);

        let tokens = ethabi::decode(&param_types, &param_data).expect("failed to decode parameters");
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
