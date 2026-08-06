use ethabi::ParamType;
use ethabi::Token;

use crate::eth::codegen;
use crate::eth::codegen::ContractName;
use crate::eth::codegen::SoliditySignature;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

pub const MAX_MULTICALL_LOGGED_SUBCALLS: usize = 32;
pub const MULTICALL_DECODE_ERROR_ABI_DECODE_FAILED: &str = "abi_decode_failed";

const DISPATCHER_CONTRACT_NAME: &str = "Dispatcher";

const SELECTOR_AGGREGATE: [u8; 4] = [0x25, 0x2d, 0xba, 0x42];
const SELECTOR_TRY_AGGREGATE: [u8; 4] = [0xbc, 0xe3, 0x8b, 0xd7];
const SELECTOR_AGGREGATE3: [u8; 4] = [0x82, 0xad, 0x56, 0xcb];
const SELECTOR_AGGREGATE3_VALUE: [u8; 4] = [0x17, 0x4d, 0xea, 0x71];

type MulticallDecodeResult<T> = Result<T, MulticallDecodeError>;

#[derive(Debug, thiserror::Error)]
pub enum MulticallDecodeError {
    #[error("Invalid multicall ABI: {source}")]
    InvalidInput {
        #[from]
        source: ethabi::Error,
    },

    #[error("Invalid multicall ABI: expected {expected} at {field}")]
    UnexpectedToken { expected: &'static str, field: &'static str },

    #[error("Invalid multicall ABI: invalid address at {field}: {source}")]
    InvalidAddress { field: &'static str, source: anyhow::Error },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MulticallKind {
    Aggregate,
    TryAggregate,
    Aggregate3,
    Aggregate3Value,
}

impl MulticallKind {
    pub fn selector(self) -> [u8; 4] {
        match self {
            Self::Aggregate => SELECTOR_AGGREGATE,
            Self::TryAggregate => SELECTOR_TRY_AGGREGATE,
            Self::Aggregate3 => SELECTOR_AGGREGATE3,
            Self::Aggregate3Value => SELECTOR_AGGREGATE3_VALUE,
        }
    }

    pub fn signature(self) -> SoliditySignature {
        match self {
            Self::Aggregate => "aggregate((address,bytes)[])",
            Self::TryAggregate => "tryAggregate(bool,(address,bytes)[])",
            Self::Aggregate3 => "aggregate3((address,bool,bytes)[])",
            Self::Aggregate3Value => "aggregate3Value((address,bool,uint256,bytes)[])",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MulticallInfo {
    pub kind: MulticallKind,
    pub parent_to: Address,
    pub parent_contract: ContractName,
    pub parent_function: SoliditySignature,
    pub total_subcalls: usize,
    pub calls: Vec<MulticallSubcall>,
    pub decode_error: Option<String>,
}

impl MulticallInfo {
    pub fn logged_calls(&self) -> &[MulticallSubcall] {
        let end = self.calls.len().min(MAX_MULTICALL_LOGGED_SUBCALLS);
        &self.calls[..end]
    }

    pub fn logged_subcalls(&self) -> usize {
        self.calls.len().min(MAX_MULTICALL_LOGGED_SUBCALLS)
    }

    pub fn decode_error_metric_label(&self) -> Option<&'static str> {
        self.decode_error.as_ref().map(|_| MULTICALL_DECODE_ERROR_ABI_DECODE_FAILED)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MulticallSubcall {
    pub index: usize,
    pub to: Address,
    pub contract: ContractName,
    pub function: SoliditySignature,
    pub allow_failure: Option<bool>,
    pub value: Option<String>,
    #[serde(skip_serializing)]
    pub input_len: usize,
}

impl MulticallSubcall {
    pub fn metric_contract(&self) -> String {
        format!("Multicall({})", self.contract)
    }
}

pub fn decode_multicall(to: Option<Address>, input: &Bytes) -> Option<MulticallInfo> {
    let to = to?;
    let parent_contract = codegen::contract_name(&Some(to));
    if parent_contract != DISPATCHER_CONTRACT_NAME {
        return None;
    }

    let selector = input.as_ref().get(..4)?.try_into().ok()?;
    let kind = kind_from_selector(selector)?;
    let parent_function = kind.signature();

    let payload = &input.as_ref()[4..];
    match decode_multicall_payload(kind, payload) {
        Ok(calls) => Some(MulticallInfo {
            kind,
            parent_to: to,
            parent_contract,
            parent_function,
            total_subcalls: calls.len(),
            calls,
            decode_error: None,
        }),
        Err(decode_error) => Some(MulticallInfo {
            kind,
            parent_to: to,
            parent_contract,
            parent_function,
            total_subcalls: 0,
            calls: Vec::new(),
            decode_error: Some(decode_error.to_string()),
        }),
    }
}

fn kind_from_selector(selector: [u8; 4]) -> Option<MulticallKind> {
    match selector {
        SELECTOR_AGGREGATE => Some(MulticallKind::Aggregate),
        SELECTOR_TRY_AGGREGATE => Some(MulticallKind::TryAggregate),
        SELECTOR_AGGREGATE3 => Some(MulticallKind::Aggregate3),
        SELECTOR_AGGREGATE3_VALUE => Some(MulticallKind::Aggregate3Value),
        _ => None,
    }
}

fn decode_multicall_payload(kind: MulticallKind, payload: &[u8]) -> MulticallDecodeResult<Vec<MulticallSubcall>> {
    match kind {
        MulticallKind::Aggregate => decode_aggregate(payload),
        MulticallKind::TryAggregate => decode_try_aggregate(payload),
        MulticallKind::Aggregate3 => decode_aggregate3(payload),
        MulticallKind::Aggregate3Value => decode_aggregate3_value(payload),
    }
}

fn decode_aggregate(payload: &[u8]) -> MulticallDecodeResult<Vec<MulticallSubcall>> {
    let tokens = ethabi::decode(&[calls_address_bytes_param()], payload)?;
    let calls = expect_array(tokens.into_iter().next(), "aggregate.calls")?;
    calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| {
            let tuple = expect_tuple(call, "aggregate.call")?;
            let to = expect_address(tuple.first(), "aggregate.call.target")?;
            let input = expect_bytes(tuple.get(1), "aggregate.call.callData")?;
            Ok(build_subcall(index, to, input, None, None))
        })
        .collect()
}

fn decode_try_aggregate(payload: &[u8]) -> MulticallDecodeResult<Vec<MulticallSubcall>> {
    let tokens = ethabi::decode(&[ParamType::Bool, calls_address_bytes_param()], payload)?;
    let require_success = expect_bool(tokens.first(), "tryAggregate.requireSuccess")?;
    let calls = expect_array(tokens.get(1).cloned(), "tryAggregate.calls")?;
    calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| {
            let tuple = expect_tuple(call, "tryAggregate.call")?;
            let to = expect_address(tuple.first(), "tryAggregate.call.target")?;
            let input = expect_bytes(tuple.get(1), "tryAggregate.call.callData")?;
            Ok(build_subcall(index, to, input, Some(!require_success), None))
        })
        .collect()
}

fn decode_aggregate3(payload: &[u8]) -> MulticallDecodeResult<Vec<MulticallSubcall>> {
    let tokens = ethabi::decode(
        &[ParamType::Array(Box::new(ParamType::Tuple(vec![
            ParamType::Address,
            ParamType::Bool,
            ParamType::Bytes,
        ])))],
        payload,
    )?;
    let calls = expect_array(tokens.into_iter().next(), "aggregate3.calls")?;
    calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| {
            let tuple = expect_tuple(call, "aggregate3.call")?;
            let to = expect_address(tuple.first(), "aggregate3.call.target")?;
            let allow_failure = expect_bool(tuple.get(1), "aggregate3.call.allowFailure")?;
            let input = expect_bytes(tuple.get(2), "aggregate3.call.callData")?;
            Ok(build_subcall(index, to, input, Some(allow_failure), None))
        })
        .collect()
}

fn decode_aggregate3_value(payload: &[u8]) -> MulticallDecodeResult<Vec<MulticallSubcall>> {
    let tokens = ethabi::decode(
        &[ParamType::Array(Box::new(ParamType::Tuple(vec![
            ParamType::Address,
            ParamType::Bool,
            ParamType::Uint(256),
            ParamType::Bytes,
        ])))],
        payload,
    )?;
    let calls = expect_array(tokens.into_iter().next(), "aggregate3Value.calls")?;
    calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| {
            let tuple = expect_tuple(call, "aggregate3Value.call")?;
            let to = expect_address(tuple.first(), "aggregate3Value.call.target")?;
            let allow_failure = expect_bool(tuple.get(1), "aggregate3Value.call.allowFailure")?;
            let value = expect_uint(tuple.get(2), "aggregate3Value.call.value")?;
            let input = expect_bytes(tuple.get(3), "aggregate3Value.call.callData")?;
            Ok(build_subcall(index, to, input, Some(allow_failure), Some(value)))
        })
        .collect()
}

fn calls_address_bytes_param() -> ParamType {
    ParamType::Array(Box::new(ParamType::Tuple(vec![ParamType::Address, ParamType::Bytes])))
}

fn build_subcall(index: usize, to: Address, input: Vec<u8>, allow_failure: Option<bool>, value: Option<String>) -> MulticallSubcall {
    let input = Bytes(input);
    MulticallSubcall {
        index,
        to,
        contract: codegen::contract_name(&Some(to)),
        function: codegen::function_sig(&input),
        allow_failure,
        value,
        input_len: input.len(),
    }
}

fn expect_array(token: Option<Token>, field: &'static str) -> MulticallDecodeResult<Vec<Token>> {
    match token {
        Some(Token::Array(tokens)) => Ok(tokens),
        _ => Err(MulticallDecodeError::UnexpectedToken { expected: "array", field }),
    }
}

fn expect_tuple(token: Token, field: &'static str) -> MulticallDecodeResult<Vec<Token>> {
    match token {
        Token::Tuple(tokens) => Ok(tokens),
        _ => Err(MulticallDecodeError::UnexpectedToken { expected: "tuple", field }),
    }
}

fn expect_address(token: Option<&Token>, field: &'static str) -> MulticallDecodeResult<Address> {
    match token {
        Some(Token::Address(address)) => {
            Address::try_from(address.as_bytes().to_vec()).map_err(|source| MulticallDecodeError::InvalidAddress { field, source })
        }
        _ => Err(MulticallDecodeError::UnexpectedToken { expected: "address", field }),
    }
}

fn expect_bool(token: Option<&Token>, field: &'static str) -> MulticallDecodeResult<bool> {
    match token {
        Some(Token::Bool(value)) => Ok(*value),
        _ => Err(MulticallDecodeError::UnexpectedToken { expected: "bool", field }),
    }
}

fn expect_bytes(token: Option<&Token>, field: &'static str) -> MulticallDecodeResult<Vec<u8>> {
    match token {
        Some(Token::Bytes(value)) => Ok(value.clone()),
        _ => Err(MulticallDecodeError::UnexpectedToken { expected: "bytes", field }),
    }
}

fn expect_uint(token: Option<&Token>, field: &'static str) -> MulticallDecodeResult<String> {
    match token {
        Some(Token::Uint(value)) => Ok(value.to_string()),
        _ => Err(MulticallDecodeError::UnexpectedToken { expected: "uint", field }),
    }
}

#[cfg(feature = "metrics")]
pub fn record_executor_multicall_subcalls(kind: &'static str, to: Option<Address>, input: &Bytes, success: bool) {
    let Some(multicall) = decode_multicall(to, input) else {
        return;
    };
    if multicall.decode_error.is_some() {
        return;
    }

    for call in multicall.logged_calls() {
        metrics::inc_executor_multicall_subcalls(
            kind,
            multicall.parent_contract,
            multicall.parent_function,
            call.metric_contract(),
            call.function,
            success,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use ethabi::Address as EthabiAddress;

    use super::*;

    fn dispatcher() -> Address {
        Address::from_str("0x597F6899E7BB0077f156ED64fd14352c2576651b").unwrap()
    }

    fn brlc() -> Address {
        Address::from_str("0xA9a55a81a4C085EC0C31585Aed4cFB09D78dfD53").unwrap()
    }

    fn unknown_address() -> Address {
        Address::from_str("0x1111111111111111111111111111111111111111").unwrap()
    }

    fn ethabi_address(address: Address) -> EthabiAddress {
        EthabiAddress::from_slice(address.as_ref())
    }

    fn transfer_input() -> Vec<u8> {
        let mut input = Vec::from([0xa9, 0x05, 0x9c, 0xbb]);
        input.extend(ethabi::encode(&[Token::Address(ethabi_address(unknown_address())), Token::Uint(100u64.into())]));
        input
    }

    fn encode_call(selector: [u8; 4], tokens: &[Token]) -> Bytes {
        let mut input = Vec::from(selector);
        input.extend(ethabi::encode(tokens));
        Bytes(input)
    }

    #[test]
    fn decode_aggregate_with_one_call() {
        let input = encode_call(
            SELECTOR_AGGREGATE,
            &[Token::Array(vec![Token::Tuple(vec![
                Token::Address(ethabi_address(brlc())),
                Token::Bytes(transfer_input()),
            ])])],
        );

        let info = decode_multicall(Some(dispatcher()), &input).unwrap();

        assert_eq!(info.parent_contract, "Dispatcher");
        assert_eq!(info.parent_function, "aggregate((address,bytes)[])");
        assert_eq!(info.total_subcalls, 1);
        assert_eq!(info.decode_error, None);
        assert_eq!(info.calls[0].index, 0);
        assert_eq!(info.calls[0].contract, "BrlcToken");
        assert_eq!(info.calls[0].metric_contract(), "Multicall(BrlcToken)");
        assert_eq!(info.calls[0].function, "transfer(address,uint256)");
        assert_eq!(info.calls[0].allow_failure, None);
        assert_eq!(info.calls[0].value, None);
    }

    #[test]
    fn decode_try_aggregate_sets_global_allow_failure() {
        let input = encode_call(
            SELECTOR_TRY_AGGREGATE,
            &[
                Token::Bool(false),
                Token::Array(vec![Token::Tuple(vec![Token::Address(ethabi_address(brlc())), Token::Bytes(transfer_input())])]),
            ],
        );

        let info = decode_multicall(Some(dispatcher()), &input).unwrap();

        assert_eq!(info.parent_function, "tryAggregate(bool,(address,bytes)[])");
        assert_eq!(info.calls[0].allow_failure, Some(true));
    }

    #[test]
    fn decode_aggregate3_sets_per_call_allow_failure() {
        let input = encode_call(
            SELECTOR_AGGREGATE3,
            &[Token::Array(vec![Token::Tuple(vec![
                Token::Address(ethabi_address(brlc())),
                Token::Bool(true),
                Token::Bytes(transfer_input()),
            ])])],
        );

        let info = decode_multicall(Some(dispatcher()), &input).unwrap();

        assert_eq!(info.parent_function, "aggregate3((address,bool,bytes)[])");
        assert_eq!(info.calls[0].allow_failure, Some(true));
    }

    #[test]
    fn decode_aggregate3_value_sets_value() {
        let input = encode_call(
            SELECTOR_AGGREGATE3_VALUE,
            &[Token::Array(vec![Token::Tuple(vec![
                Token::Address(ethabi_address(brlc())),
                Token::Bool(false),
                Token::Uint(123u64.into()),
                Token::Bytes(transfer_input()),
            ])])],
        );

        let info = decode_multicall(Some(dispatcher()), &input).unwrap();

        assert_eq!(info.parent_function, "aggregate3Value((address,bool,uint256,bytes)[])");
        assert_eq!(info.calls[0].allow_failure, Some(false));
        assert_eq!(info.calls[0].value.as_deref(), Some("123"));
    }

    #[test]
    fn decode_returns_none_for_non_dispatcher() {
        let input = encode_call(SELECTOR_AGGREGATE, &[Token::Array(vec![])]);

        assert!(decode_multicall(Some(brlc()), &input).is_none());
    }

    #[test]
    fn decode_returns_none_for_unknown_selector() {
        let input = Bytes(vec![0xff, 0xff, 0xff, 0xff]);

        assert!(decode_multicall(Some(dispatcher()), &input).is_none());
    }

    #[test]
    fn decode_returns_error_for_malformed_known_selector() {
        let input = Bytes(Vec::from(SELECTOR_AGGREGATE));

        let info = decode_multicall(Some(dispatcher()), &input).unwrap();

        assert!(info.decode_error.is_some());
        assert!(info.decode_error.as_deref().unwrap().contains("Invalid multicall ABI"));
        assert_eq!(info.decode_error_metric_label(), Some("abi_decode_failed"));
        assert_eq!(info.total_subcalls, 0);
        assert!(info.calls.is_empty());
    }

    #[test]
    fn unexpected_token_returns_typed_error() {
        let result = expect_bool(Some(&Token::Bytes(Vec::new())), "test.bool");

        assert!(matches!(
            result,
            Err(MulticallDecodeError::UnexpectedToken {
                expected: "bool",
                field: "test.bool",
            })
        ));
    }

    #[test]
    fn decode_unknown_target_and_empty_input_to_existing_labels() {
        let input = encode_call(
            SELECTOR_AGGREGATE,
            &[Token::Array(vec![Token::Tuple(vec![
                Token::Address(ethabi_address(unknown_address())),
                Token::Bytes(Vec::new()),
            ])])],
        );

        let info = decode_multicall(Some(dispatcher()), &input).unwrap();

        assert_eq!(info.calls[0].contract, "unknown");
        assert_eq!(info.calls[0].function, "missing");
    }

    #[test]
    fn logged_calls_is_bounded_to_32_but_total_keeps_full_count() {
        let calls = (0..40)
            .map(|_| Token::Tuple(vec![Token::Address(ethabi_address(brlc())), Token::Bytes(transfer_input())]))
            .collect();
        let input = encode_call(SELECTOR_AGGREGATE, &[Token::Array(calls)]);

        let info = decode_multicall(Some(dispatcher()), &input).unwrap();

        assert_eq!(info.total_subcalls, 40);
        assert_eq!(info.logged_subcalls(), 32);
        assert_eq!(info.logged_calls().len(), 32);
    }
}
