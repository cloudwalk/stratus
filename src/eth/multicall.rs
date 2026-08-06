use alloy_sol_types::SolCall;
use alloy_sol_types::sol;

use crate::eth::codegen;
use crate::eth::codegen::ContractName;
use crate::eth::codegen::SoliditySignature;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::MulticallError;

pub const MAX_MULTICALL_LOGGED_SUBCALLS: usize = 32;
pub const MULTICALL_CONTRACT_NAME: ContractName = "Multicall";

const DISPATCHER_CONTRACT_NAME: &str = "Dispatcher";
const DISPATCHER_ADDRESS: Address = Address::new([
    0x59, 0x7f, 0x68, 0x99, 0xe7, 0xbb, 0x00, 0x77, 0xf1, 0x56, 0xed, 0x64, 0xfd, 0x14, 0x35, 0x2c, 0x25, 0x76, 0x65, 0x1b,
]);

sol! {
    struct Call {
        address target;
        bytes callData;
    }

    struct Call3 {
        address target;
        bool allowFailure;
        bytes callData;
    }

    struct Call3Value {
        address target;
        bool allowFailure;
        uint256 value;
        bytes callData;
    }

    function aggregate(Call[] calls) external;
    function tryAggregate(bool requireSuccess, Call[] calls) external;
    function aggregate3(Call3[] calls) external;
    function aggregate3Value(Call3Value[] calls) external;
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MulticallKind {
    Aggregate,
    TryAggregate,
    Aggregate3,
    Aggregate3Value,
}

#[derive(Debug, Clone)]
pub struct MulticallInfo {
    pub kind: MulticallKind,
    pub parent_to: Address,
    pub parent_contract: ContractName,
    pub parent_function: SoliditySignature,
    pub total_subcalls: usize,
    pub subcalls: Vec<MulticallSubcall>,
    pub decode_error: Option<String>,
}

impl MulticallInfo {
    fn new(kind: MulticallKind, parent_to: Address, parent_input: &Bytes, subcalls: Vec<MulticallSubcall>) -> Self {
        Self {
            kind,
            parent_to,
            parent_contract: DISPATCHER_CONTRACT_NAME,
            parent_function: codegen::function_sig(parent_input),
            total_subcalls: subcalls.len(),
            subcalls,
            decode_error: None,
        }
    }

    fn from_decode_error(kind: MulticallKind, parent_to: Address, parent_input: &Bytes, decode_error: MulticallError) -> Self {
        Self {
            kind,
            parent_to,
            parent_contract: DISPATCHER_CONTRACT_NAME,
            parent_function: codegen::function_sig(parent_input),
            total_subcalls: 0,
            subcalls: Vec::new(),
            decode_error: Some(decode_error.to_string()),
        }
    }

    pub fn logged_subcalls(&self) -> &[MulticallSubcall] {
        let end = self.subcalls.len().min(MAX_MULTICALL_LOGGED_SUBCALLS);
        &self.subcalls[..end]
    }

    pub fn logged_subcalls_count(&self) -> usize {
        self.subcalls.len().min(MAX_MULTICALL_LOGGED_SUBCALLS)
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
    fn new(index: usize, to: Address, input: Vec<u8>, allow_failure: Option<bool>, value: Option<String>) -> Self {
        let input = Bytes(input);
        Self {
            index,
            to,
            contract: codegen::contract_name(&Some(to)),
            function: codegen::function_sig(&input),
            allow_failure,
            value,
            input_len: input.len(),
        }
    }

    pub fn metric_contract(&self) -> String {
        format!("Multicall({})", self.contract)
    }

    pub fn metric_function(&self, parent_function: SoliditySignature) -> String {
        format!("{}({})", parent_function, self.function)
    }
}

pub fn is_multicall_contract(to: Address) -> bool {
    if to != DISPATCHER_ADDRESS {
        return false;
    }
    true
}

pub fn decode_multicall(to: Address, input: &Bytes) -> Option<MulticallInfo> {
    if !is_multicall_contract(to) {
        return None;
    }
    let selector = input.as_ref().get(..4)?.try_into().ok()?;
    let kind = kind_from_selector(selector)?;

    match decode_multicall_payload(kind, input.as_ref()) {
        Ok(subcalls) => Some(MulticallInfo::new(kind, to, input, subcalls)),
        Err(decode_error) => Some(MulticallInfo::from_decode_error(kind, to, input, decode_error)),
    }
}

fn kind_from_selector(selector: [u8; 4]) -> Option<MulticallKind> {
    match selector {
        aggregateCall::SELECTOR => Some(MulticallKind::Aggregate),
        tryAggregateCall::SELECTOR => Some(MulticallKind::TryAggregate),
        aggregate3Call::SELECTOR => Some(MulticallKind::Aggregate3),
        aggregate3ValueCall::SELECTOR => Some(MulticallKind::Aggregate3Value),
        _ => None,
    }
}

fn decode_multicall_payload(kind: MulticallKind, input: &[u8]) -> anyhow::Result<Vec<MulticallSubcall>, MulticallError> {
    match kind {
        MulticallKind::Aggregate => decode_aggregate(input),
        MulticallKind::TryAggregate => decode_try_aggregate(input),
        MulticallKind::Aggregate3 => decode_aggregate3(input),
        MulticallKind::Aggregate3Value => decode_aggregate3_value(input),
    }
}

fn decode_aggregate(input: &[u8]) -> anyhow::Result<Vec<MulticallSubcall>, MulticallError> {
    let call = aggregateCall::abi_decode(input)?;
    Ok(call
        .calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| MulticallSubcall::new(index, decode_address(call.target), call.callData.into(), None, None))
        .collect())
}

fn decode_try_aggregate(input: &[u8]) -> anyhow::Result<Vec<MulticallSubcall>, MulticallError> {
    let call = tryAggregateCall::abi_decode(input)?;
    Ok(call
        .calls
        .into_iter()
        .enumerate()
        .map(|(index, subcall)| MulticallSubcall::new(index, decode_address(subcall.target), subcall.callData.into(), Some(!call.requireSuccess), None))
        .collect())
}

fn decode_aggregate3(input: &[u8]) -> anyhow::Result<Vec<MulticallSubcall>, MulticallError> {
    let call = aggregate3Call::abi_decode(input)?;
    Ok(call
        .calls
        .into_iter()
        .enumerate()
        .map(|(index, subcall)| MulticallSubcall::new(index, decode_address(subcall.target), subcall.callData.into(), Some(subcall.allowFailure), None))
        .collect())
}

fn decode_aggregate3_value(input: &[u8]) -> anyhow::Result<Vec<MulticallSubcall>, MulticallError> {
    let call = aggregate3ValueCall::abi_decode(input)?;
    Ok(call
        .calls
        .into_iter()
        .enumerate()
        .map(|(index, subcall)| {
            MulticallSubcall::new(
                index,
                decode_address(subcall.target),
                subcall.callData.into(),
                Some(subcall.allowFailure),
                Some(subcall.value.to_string()),
            )
        })
        .collect())
}

fn decode_address(address: alloy_primitives::Address) -> Address {
    Address::from(address.0)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy_primitives::U256;
    use alloy_sol_types::sol;

    use super::*;

    sol! {
        function transfer(address,uint256) external returns (bool);
    }

    fn dispatcher() -> Address {
        Address::from_str("0x597F6899E7BB0077f156ED64fd14352c2576651b").unwrap()
    }

    fn brlc() -> Address {
        Address::from_str("0xA9a55a81a4C085EC0C31585Aed4cFB09D78dfD53").unwrap()
    }

    fn unknown_address() -> Address {
        Address::from_str("0x1111111111111111111111111111111111111111").unwrap()
    }

    fn alloy_address(address: Address) -> alloy_primitives::Address {
        alloy_primitives::Address::from_slice(address.as_ref())
    }

    fn transfer_input() -> Vec<u8> {
        transferCall {
            _0: alloy_address(unknown_address()),
            _1: U256::from(100),
        }
        .abi_encode()
    }

    #[test]
    fn decode_aggregate_with_one_call() {
        let input = Bytes(
            aggregateCall {
                calls: vec![Call {
                    target: alloy_address(brlc()),
                    callData: transfer_input().into(),
                }],
            }
            .abi_encode(),
        );

        let info = decode_multicall(dispatcher(), &input).unwrap();

        assert_eq!(info.parent_contract, "Dispatcher");
        assert_eq!(info.parent_function, "aggregate((address,bytes)[])");
        assert_eq!(info.total_subcalls, 1);
        assert_eq!(info.decode_error, None);
        assert_eq!(info.subcalls[0].index, 0);
        assert_eq!(info.subcalls[0].contract, "BrlcToken");
        assert_eq!(info.subcalls[0].metric_contract(), "Multicall(BrlcToken)");
        assert_eq!(
            info.subcalls[0].metric_function(info.parent_function),
            "aggregate((address,bytes)[])(transfer(address,uint256))"
        );
        assert_eq!(info.subcalls[0].function, "transfer(address,uint256)");
        assert_eq!(info.subcalls[0].allow_failure, None);
        assert_eq!(info.subcalls[0].value, None);
    }

    #[test]
    fn decode_try_aggregate_sets_global_allow_failure() {
        let input = Bytes(
            tryAggregateCall {
                requireSuccess: false,
                calls: vec![Call {
                    target: alloy_address(brlc()),
                    callData: transfer_input().into(),
                }],
            }
            .abi_encode(),
        );

        let info = decode_multicall(dispatcher(), &input).unwrap();

        assert_eq!(info.parent_function, "tryAggregate(bool,(address,bytes)[])");
        assert_eq!(info.subcalls[0].allow_failure, Some(true));
    }

    #[test]
    fn decode_aggregate3_sets_per_call_allow_failure() {
        let input = Bytes(
            aggregate3Call {
                calls: vec![Call3 {
                    target: alloy_address(brlc()),
                    allowFailure: true,
                    callData: transfer_input().into(),
                }],
            }
            .abi_encode(),
        );

        let info = decode_multicall(dispatcher(), &input).unwrap();

        assert_eq!(info.parent_function, "aggregate3((address,bool,bytes)[])");
        assert_eq!(info.subcalls[0].allow_failure, Some(true));
    }

    #[test]
    fn decode_aggregate3_value_sets_value() {
        let input = Bytes(
            aggregate3ValueCall {
                calls: vec![Call3Value {
                    target: alloy_address(brlc()),
                    allowFailure: false,
                    value: U256::from(123),
                    callData: transfer_input().into(),
                }],
            }
            .abi_encode(),
        );

        let info = decode_multicall(dispatcher(), &input).unwrap();

        assert_eq!(info.parent_function, "aggregate3Value((address,bool,uint256,bytes)[])");
        assert_eq!(info.subcalls[0].allow_failure, Some(false));
        assert_eq!(info.subcalls[0].value.as_deref(), Some("123"));
    }

    #[test]
    fn decode_returns_none_for_non_dispatcher() {
        let input = Bytes(aggregateCall { calls: Vec::new() }.abi_encode());

        assert!(!is_multicall_contract(brlc()));
        assert!(decode_multicall(brlc(), &input).is_none());
    }

    #[test]
    fn decode_returns_none_for_unknown_selector() {
        let input = Bytes(vec![0xff, 0xff, 0xff, 0xff]);

        assert!(decode_multicall(dispatcher(), &input).is_none());
    }

    #[test]
    fn decode_returns_error_for_malformed_known_selector() {
        let input = Bytes(Vec::from(aggregateCall::SELECTOR));

        let info = decode_multicall(dispatcher(), &input).unwrap();

        assert!(info.decode_error.is_some());
        assert!(info.decode_error.as_deref().unwrap().contains("invalid multicall ABI"));
        assert_eq!(info.total_subcalls, 0);
        assert!(info.subcalls.is_empty());
    }

    #[test]
    fn decode_unknown_target_and_empty_input_to_existing_labels() {
        let input = Bytes(
            aggregateCall {
                calls: vec![Call {
                    target: alloy_address(unknown_address()),
                    callData: Vec::new().into(),
                }],
            }
            .abi_encode(),
        );

        let info = decode_multicall(dispatcher(), &input).unwrap();

        assert_eq!(info.subcalls[0].contract, "unknown");
        assert_eq!(info.subcalls[0].function, "missing");
    }

    #[test]
    fn logged_subcalls_are_bounded_to_32_but_total_keeps_full_count() {
        let calls = (0..40)
            .map(|_| Call {
                target: alloy_address(brlc()),
                callData: transfer_input().into(),
            })
            .collect();
        let input = Bytes(aggregateCall { calls }.abi_encode());

        let info = decode_multicall(dispatcher(), &input).unwrap();

        assert_eq!(info.total_subcalls, 40);
        assert_eq!(info.logged_subcalls_count(), 32);
        assert_eq!(info.logged_subcalls().len(), 32);
    }
}
