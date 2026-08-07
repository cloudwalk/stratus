use alloy_primitives::FixedBytes;
use alloy_sol_types::SolCall;
use alloy_sol_types::SolInterface;
use alloy_sol_types::sol;
use hex_literal::hex;

use crate::eth::codegen;
use crate::eth::codegen::ContractName;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;

pub const MAX_MULTICALL_LOGGED_SUBCALLS: usize = 32;
pub const MULTICALL_CONTRACT_NAME: ContractName = "Multicall3";

const MULTICALL_ADDRESS: Address = Address(FixedBytes(hex!("ca11bde05977b3631167028862be2a173976ca11")));

sol!(Multicall, "static/contracts-abi/Multicall3.json");

#[derive(Debug, Clone)]
pub struct MulticallInfo {
    pub total_subcalls: usize,
    pub subcalls: Vec<MulticallSubcall>,
}

impl MulticallInfo {
    fn new(subcalls: Vec<MulticallSubcall>) -> Self {
        Self {
            total_subcalls: subcalls.len(),
            subcalls,
        }
    }

    pub fn logged_subcalls(&self) -> &[MulticallSubcall] {
        let end = self.logged_subcalls_count();
        &self.subcalls[..end]
    }

    pub fn logged_subcalls_count(&self) -> usize {
        self.subcalls.len().min(MAX_MULTICALL_LOGGED_SUBCALLS)
    }
}

impl TryFrom<Multicall::MulticallCalls> for MulticallInfo {
    type Error = anyhow::Error;

    fn try_from(value: Multicall::MulticallCalls) -> Result<Self, Self::Error> {
        match value {
            Multicall::MulticallCalls::aggregate(call) => Ok(Self::new(subcalls_from_calls(call.calls, None))),
            Multicall::MulticallCalls::tryAggregate(call) => Ok(Self::new(subcalls_from_calls(call.calls, Some(!call.requireSuccess)))),
            Multicall::MulticallCalls::aggregate3(call) => Ok(Self::new(subcalls_from_call3(call.calls))),
            Multicall::MulticallCalls::aggregate3Value(call) => Ok(Self::new(subcalls_from_call3_value(call.calls))),
            Multicall::MulticallCalls::blockAndAggregate(call) => Ok(Self::new(subcalls_from_calls(call.calls, None))),
            Multicall::MulticallCalls::tryBlockAndAggregate(call) => Ok(Self::new(subcalls_from_calls(call.calls, Some(!call.requireSuccess)))),
            _ => anyhow::bail!("unsupported multicall function"),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MulticallSubcall {
    pub index: usize,
    pub to: Address,
    pub contract: ContractName,
    pub function: codegen::SoliditySignature,
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

    pub fn metric_function(&self, parent_function: codegen::SoliditySignature) -> String {
        format!("{}({})", parent_function, self.function)
    }
}

pub fn is_multicall_contract(to: Address) -> bool {
    to == MULTICALL_ADDRESS
}

pub fn is_multicall_subcall_method(input: &Bytes) -> bool {
    let Some(selector) = input.as_ref().get(..4).and_then(|selector| selector.try_into().ok()) else {
        return false;
    };

    matches!(
        selector,
        Multicall::aggregateCall::SELECTOR
            | Multicall::tryAggregateCall::SELECTOR
            | Multicall::aggregate3Call::SELECTOR
            | Multicall::aggregate3ValueCall::SELECTOR
            | Multicall::blockAndAggregateCall::SELECTOR
            | Multicall::tryBlockAndAggregateCall::SELECTOR
    )
}

pub fn decode_multicall(input: &Bytes) -> anyhow::Result<MulticallInfo> {
    let decoded = Multicall::MulticallCalls::abi_decode(input.as_ref()).map_err(crate::eth::primitives::MulticallError::from)?;
    decoded.try_into()
}

fn subcalls_from_calls(calls: Vec<Multicall3::Call>, allow_failure: Option<bool>) -> Vec<MulticallSubcall> {
    calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| MulticallSubcall::new(index, call.target.into(), call.callData.into(), allow_failure, None))
        .collect()
}

fn subcalls_from_call3(calls: Vec<Multicall3::Call3>) -> Vec<MulticallSubcall> {
    calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| MulticallSubcall::new(index, call.target.into(), call.callData.into(), Some(call.allowFailure), None))
        .collect()
}

fn subcalls_from_call3_value(calls: Vec<Multicall3::Call3Value>) -> Vec<MulticallSubcall> {
    calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| {
            MulticallSubcall::new(
                index,
                call.target.into(),
                call.callData.into(),
                Some(call.allowFailure),
                Some(call.value.to_string()),
            )
        })
        .collect()
}

#[cfg(test)]
fn alloy_address(address: Address) -> alloy_primitives::Address {
    address.into()
}

#[cfg(test)]
fn multicall_address() -> Address {
    MULTICALL_ADDRESS
}

#[cfg(test)]
fn brlc_address() -> Address {
    Address::BRLC
}

#[cfg(test)]
fn unknown_address() -> Address {
    Address::new([0x11; 20])
}

#[cfg(test)]
fn transfer_input() -> Vec<u8> {
    use alloy_primitives::U256;
    use alloy_sol_types::sol;

    sol! {
        function transfer(address,uint256) external returns (bool);
    }

    transferCall {
        _0: alloy_address(unknown_address()),
        _1: U256::from(100),
    }
    .abi_encode()
}

#[cfg(test)]
mod tests {
    use alloy_primitives::U256;

    use super::*;

    #[test]
    fn decode_aggregate_with_one_call() {
        let input = Bytes(
            Multicall::aggregateCall {
                calls: vec![Multicall3::Call {
                    target: alloy_address(brlc_address()),
                    callData: transfer_input().into(),
                }],
            }
            .abi_encode(),
        );

        let info = decode_multicall(&input).unwrap();

        assert_eq!(info.total_subcalls, 1);
        assert_eq!(info.subcalls[0].index, 0);
        assert_eq!(info.subcalls[0].contract, "BrlcToken");
        assert_eq!(info.subcalls[0].metric_contract(), "Multicall(BrlcToken)");
        assert_eq!(
            info.subcalls[0].metric_function(Multicall::aggregateCall::SIGNATURE),
            "aggregate((address,bytes)[])(transfer(address,uint256))"
        );
        assert_eq!(info.subcalls[0].function, "transfer(address,uint256)");
        assert_eq!(info.subcalls[0].allow_failure, None);
        assert_eq!(info.subcalls[0].value, None);
    }

    #[test]
    fn decode_try_aggregate_sets_global_allow_failure() {
        let input = Bytes(
            Multicall::tryAggregateCall {
                requireSuccess: false,
                calls: vec![Multicall3::Call {
                    target: alloy_address(brlc_address()),
                    callData: transfer_input().into(),
                }],
            }
            .abi_encode(),
        );

        let info = decode_multicall(&input).unwrap();

        assert_eq!(info.subcalls[0].allow_failure, Some(true));
    }

    #[test]
    fn decode_aggregate3_sets_per_call_allow_failure() {
        let input = Bytes(
            Multicall::aggregate3Call {
                calls: vec![Multicall3::Call3 {
                    target: alloy_address(brlc_address()),
                    allowFailure: true,
                    callData: transfer_input().into(),
                }],
            }
            .abi_encode(),
        );

        let info = decode_multicall(&input).unwrap();

        assert_eq!(info.subcalls[0].allow_failure, Some(true));
    }

    #[test]
    fn decode_aggregate3_value_sets_value() {
        let input = Bytes(
            Multicall::aggregate3ValueCall {
                calls: vec![Multicall3::Call3Value {
                    target: alloy_address(brlc_address()),
                    allowFailure: false,
                    value: U256::from(123),
                    callData: transfer_input().into(),
                }],
            }
            .abi_encode(),
        );

        let info = decode_multicall(&input).unwrap();

        assert_eq!(info.subcalls[0].allow_failure, Some(false));
        assert_eq!(info.subcalls[0].value.as_deref(), Some("123"));
    }

    #[test]
    fn decode_block_and_aggregate_with_one_call() {
        let input = Bytes(
            Multicall::blockAndAggregateCall {
                calls: vec![Multicall3::Call {
                    target: alloy_address(brlc_address()),
                    callData: transfer_input().into(),
                }],
            }
            .abi_encode(),
        );

        let info = decode_multicall(&input).unwrap();

        assert_eq!(info.total_subcalls, 1);
        assert_eq!(info.subcalls[0].contract, "BrlcToken");
    }

    #[test]
    fn decode_try_block_and_aggregate_sets_global_allow_failure() {
        let input = Bytes(
            Multicall::tryBlockAndAggregateCall {
                requireSuccess: true,
                calls: vec![Multicall3::Call {
                    target: alloy_address(brlc_address()),
                    callData: transfer_input().into(),
                }],
            }
            .abi_encode(),
        );

        let info = decode_multicall(&input).unwrap();

        assert_eq!(info.subcalls[0].allow_failure, Some(false));
    }

    #[test]
    fn selector_helpers_filter_decode_candidates() {
        let aggregate = Bytes(Multicall::aggregateCall { calls: Vec::new() }.abi_encode());
        let get_block_number = Bytes(Multicall::getBlockNumberCall {}.abi_encode());
        let unknown = Bytes(vec![0xff, 0xff, 0xff, 0xff]);

        assert!(is_multicall_contract(multicall_address()));
        assert!(!is_multicall_contract(brlc_address()));
        assert!(is_multicall_subcall_method(&aggregate));
        assert!(!is_multicall_subcall_method(&get_block_number));
        assert!(!is_multicall_subcall_method(&unknown));
    }

    #[test]
    fn decode_returns_error_for_malformed_known_selector() {
        let input = Bytes(Vec::from(Multicall::aggregateCall::SELECTOR));

        let error = decode_multicall(&input).unwrap_err();

        assert!(error.to_string().contains("invalid multicall ABI"));
    }

    #[test]
    fn decode_unknown_target_and_empty_input_to_existing_labels() {
        let input = Bytes(
            Multicall::aggregateCall {
                calls: vec![Multicall3::Call {
                    target: alloy_address(unknown_address()),
                    callData: Vec::new().into(),
                }],
            }
            .abi_encode(),
        );

        let info = decode_multicall(&input).unwrap();

        assert_eq!(info.subcalls[0].contract, "unknown");
        assert_eq!(info.subcalls[0].function, "missing");
    }

    #[test]
    fn logged_subcalls_are_bounded_to_32_but_total_keeps_full_count() {
        let calls = (0..40)
            .map(|_| Multicall3::Call {
                target: alloy_address(brlc_address()),
                callData: transfer_input().into(),
            })
            .collect();
        let input = Bytes(Multicall::aggregateCall { calls }.abi_encode());

        let info = decode_multicall(&input).unwrap();

        assert_eq!(info.total_subcalls, 40);
        assert_eq!(info.logged_subcalls_count(), 32);
        assert_eq!(info.logged_subcalls().len(), 32);
    }
}
