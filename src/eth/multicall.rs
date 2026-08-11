#[cfg(feature = "metrics")]
use std::time::Duration;

use alloy_primitives::FixedBytes;
use alloy_sol_types::SolCall;
use alloy_sol_types::SolInterface;
use alloy_sol_types::sol;
use hex_literal::hex;

use crate::eth::codegen;
use crate::eth::codegen::ContractName;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::MulticallError;
#[cfg(feature = "metrics")]
use crate::eth::rpc::RpcClientApp;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

pub const MAX_MULTICALL_LOGGED_SUBCALLS: usize = 32;
pub const MULTICALL_CONTRACT_NAME: ContractName = "Multicall3";

const MULTICALL_ADDRESS: Address = Address(FixedBytes(hex!("ca11bde05977b3631167028862be2a173976ca11")));

sol!(Multicall, "static/contracts-abi/Multicall3.json");

#[derive(Debug, Clone)]
pub struct MulticallInfo {
    pub total_subcalls: usize,
    pub subcalls: Vec<MulticallSubcall>,
    pub parent_function: codegen::SoliditySignature,
}

impl MulticallInfo {
    pub fn decode_opt(to: Option<Address>, input: &Bytes) -> Option<Self> {
        to.is_some_and(is_multicall_contract)
            .then(|| Multicall::MulticallCalls::abi_decode(input.as_ref()))?
            .map_err(MulticallError::from)
            .and_then(TryInto::try_into)
            .inspect_err(|err| tracing::warn!(reason = %err, "failed to decode multicall input"))
            .ok()
    }

    fn from_subcalls(parent_function: codegen::SoliditySignature, subcalls: Vec<MulticallSubcall>) -> Self {
        Self {
            total_subcalls: subcalls.len(),
            subcalls,
            parent_function,
        }
    }

    #[cfg(feature = "metrics")]
    pub fn record_rpc_requests_started(&self, client: &RpcClientApp, method: &str, req_type: impl Into<metrics::MetricLabelValue> + Copy) {
        for subcall in self.logged_subcalls() {
            metrics::inc_rpc_requests_started(
                client,
                method,
                subcall.metric_contract(),
                subcall.metric_function(self.parent_function),
                req_type,
            );
        }
    }

    #[cfg(feature = "metrics")]
    pub fn record_rpc_requests_finished(&self, elapsed: Duration, client: &RpcClientApp, method: &str, rpc_result: &str, result_code: i32, success: bool) {
        for subcall in self.logged_subcalls() {
            metrics::inc_rpc_requests_finished(
                elapsed,
                client,
                method,
                subcall.metric_contract(),
                subcall.metric_function(self.parent_function),
                rpc_result,
                result_code,
                success,
            );
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
    type Error = MulticallError;

    fn try_from(value: Multicall::MulticallCalls) -> Result<Self, Self::Error> {
        let (parent_function, subcalls) = match value {
            Multicall::MulticallCalls::aggregate(call) => (Multicall::aggregateCall::SIGNATURE, subcalls_from_calls(call.calls, None)),
            Multicall::MulticallCalls::tryAggregate(call) => (
                Multicall::tryAggregateCall::SIGNATURE,
                subcalls_from_calls(call.calls, Some(!call.requireSuccess)),
            ),
            Multicall::MulticallCalls::aggregate3(call) => (Multicall::aggregate3Call::SIGNATURE, subcalls_from_calls(call.calls, None)),
            Multicall::MulticallCalls::aggregate3Value(call) => (Multicall::aggregate3ValueCall::SIGNATURE, subcalls_from_calls(call.calls, None)),
            Multicall::MulticallCalls::blockAndAggregate(call) => (Multicall::blockAndAggregateCall::SIGNATURE, subcalls_from_calls(call.calls, None)),
            Multicall::MulticallCalls::tryBlockAndAggregate(call) => (
                Multicall::tryBlockAndAggregateCall::SIGNATURE,
                subcalls_from_calls(call.calls, Some(!call.requireSuccess)),
            ),
            _non_subcall_call => return Err(MulticallError::UnsupportedMulticallFunction),
        };

        Ok(Self::from_subcalls(parent_function, subcalls))
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

trait MulticallCall {
    fn target(&self) -> Address;

    fn data(&self) -> &[u8];

    fn allow_failure(&self) -> Option<bool> {
        None
    }

    fn value(&self) -> Option<String> {
        None
    }

    fn into_multicall_subcall(self, index: usize, allow_failure: Option<bool>) -> MulticallSubcall
    where
        Self: Sized,
    {
        MulticallSubcall::new(
            index,
            self.target(),
            self.data().to_vec(),
            allow_failure.or_else(|| self.allow_failure()),
            self.value(),
        )
    }
}

impl MulticallCall for Multicall3::Call {
    fn target(&self) -> Address {
        self.target.into()
    }

    fn data(&self) -> &[u8] {
        self.callData.as_ref()
    }
}

impl MulticallCall for Multicall3::Call3 {
    fn target(&self) -> Address {
        self.target.into()
    }

    fn data(&self) -> &[u8] {
        self.callData.as_ref()
    }

    fn allow_failure(&self) -> Option<bool> {
        Some(self.allowFailure)
    }
}

impl MulticallCall for Multicall3::Call3Value {
    fn target(&self) -> Address {
        self.target.into()
    }

    fn data(&self) -> &[u8] {
        self.callData.as_ref()
    }

    fn allow_failure(&self) -> Option<bool> {
        Some(self.allowFailure)
    }

    fn value(&self) -> Option<String> {
        Some(self.value.to_string())
    }
}

fn subcalls_from_calls<T>(calls: Vec<T>, allow_failure: Option<bool>) -> Vec<MulticallSubcall>
where
    T: MulticallCall,
{
    calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| call.into_multicall_subcall(index, allow_failure))
        .collect()
}

#[cfg(test)]
mod tests {
    use alloy_primitives::U256;

    use super::*;

    fn alloy_address(address: Address) -> alloy_primitives::Address {
        address.into()
    }

    fn multicall_address() -> Address {
        MULTICALL_ADDRESS
    }

    fn brlc_address() -> Address {
        Address::BRLC
    }

    fn unknown_address() -> Address {
        Address::new([0x11; 20])
    }

    fn transfer_input() -> Vec<u8> {
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

    fn multicall_info(input: &Bytes) -> Option<MulticallInfo> {
        MulticallInfo::decode_opt(Some(multicall_address()), input)
    }

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

        let info = multicall_info(&input).unwrap();

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

        let info = multicall_info(&input).unwrap();

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

        let info = multicall_info(&input).unwrap();

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

        let info = multicall_info(&input).unwrap();

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

        let info = multicall_info(&input).unwrap();

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

        let info = multicall_info(&input).unwrap();

        assert_eq!(info.subcalls[0].allow_failure, Some(false));
    }

    #[test]
    fn decode_opt_returns_none_for_malformed_known_selector() {
        let input = Bytes(Vec::from(Multicall::aggregateCall::SELECTOR));

        assert!(multicall_info(&input).is_none());
    }

    #[test]
    fn unknown_selector_is_decode_error() {
        let input = Bytes(vec![0xff; 4]);
        let err = match Multicall::MulticallCalls::abi_decode(input.as_ref()).map_err(MulticallError::from) {
            Ok(_) => panic!("expected invalid input error"),
            Err(err) => err,
        };

        assert!(matches!(
            err,
            MulticallError::DecodeError {
                source: alloy_sol_types::Error::UnknownSelector { .. },
            }
        ));
    }

    #[test]
    fn known_non_subcall_function_is_unsupported() {
        let err = MulticallInfo::try_from(Multicall::MulticallCalls::getBasefee(Multicall::getBasefeeCall {})).unwrap_err();

        assert!(matches!(err, MulticallError::UnsupportedMulticallFunction));
    }

    #[test]
    fn decode_opt_returns_none_for_non_multicall_contract() {
        let input = Bytes(Multicall::aggregateCall { calls: Vec::new() }.abi_encode());

        assert!(MulticallInfo::decode_opt(Some(brlc_address()), &input).is_none());
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

        let info = multicall_info(&input).unwrap();

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

        let info = multicall_info(&input).unwrap();

        assert_eq!(info.total_subcalls, 40);
        assert_eq!(info.logged_subcalls_count(), 32);
        assert_eq!(info.logged_subcalls().len(), 32);
    }
}
