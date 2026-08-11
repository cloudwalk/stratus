use std::mem;

use alloy_primitives::U256;
use alloy_rpc_types_trace::geth::CallFrame;
use alloy_rpc_types_trace::geth::FourByteFrame;
use alloy_rpc_types_trace::geth::GethDebugBuiltInTracerType;
use alloy_rpc_types_trace::geth::GethDebugTracerType;
use alloy_rpc_types_trace::geth::GethTrace;
use alloy_rpc_types_trace::geth::NoopFrame;
use alloy_rpc_types_trace::geth::call::FlatCallFrame;
use alloy_rpc_types_trace::geth::mux::MuxFrame;
use itertools::Itertools;
use revm::Context;
use revm::Database;
use revm::context::BlockEnv;
use revm::context::Evm as RevmEvm;
use revm::context::TransactTo;
use revm::context::TxEnv;
use revm::context::result::ExecutionResult as RevmExecutionResult;
use revm::context::result::ResultAndState;
use revm::handler::EthPrecompiles;
use revm::handler::instructions::EthInstructions;
use revm::primitives::hardfork::SpecId;
use revm::state::EvmState;

use crate::eth::codegen;
use crate::eth::executor::EvmKind;
use crate::eth::executor::ExecutionInput;
use crate::eth::executor::evm::GeneralRevm;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Complete;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Log;
use crate::eth::primitives::Slot;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionExecution;
use crate::ext::OptionExt;

/// Maximum gas limit allowed for a transaction. Prevents a transaction from consuming too many resources.
#[cfg(feature = "dev")]
const GAS_MAX_LIMIT: u64 = 1_000_000_000;
#[cfg(not(feature = "dev"))]
const GAS_MAX_LIMIT: u64 = 100_000_000;

pub fn parse_revm_result_and_state(revm_result: ResultAndState) -> Result<EvmExecution, StratusError> {
    let (result, tx_output, logs, gas) = parse_revm_result(revm_result.result);
    let (changes, deployed_contract_address) = parse_revm_state(revm_result.state)?;
    tracing::debug!(?result, %gas, tx_output_len = %tx_output.len(), %tx_output, "evm executed");

    Ok(EvmExecution {
        result,
        output: tx_output,
        logs,
        gas_used: gas,
        changes,
        deployed_contract_address,
    })
}

fn parse_revm_result(result: RevmExecutionResult) -> (ExecutionResult, Bytes, Vec<Log>, Gas) {
    match result {
        RevmExecutionResult::Success { output, gas, logs, .. } => {
            let result = ExecutionResult::Success;
            let output = Bytes::from(output);
            let logs = logs.into_iter().map_into().collect();
            let gas = Gas::from(gas);
            (result, output, logs, gas)
        }
        RevmExecutionResult::Revert { output, gas, logs } => {
            let output = Bytes::from(output);
            let result = ExecutionResult::Reverted { reason: (&output).into() };
            let gas = Gas::from(gas);
            let logs = logs.into_iter().map_into().collect();
            (result, output, logs, gas)
        }
        RevmExecutionResult::Halt { reason, gas, logs } => {
            let result = ExecutionResult::new_halted(format!("{reason:?}"));
            let output = Bytes::default();
            let gas = Gas::from(gas);
            let logs = logs.into_iter().map_into().collect();
            (result, output, logs, gas)
        }
    }
}

fn parse_revm_state(revm_state: EvmState) -> Result<(ExecutionChanges, Option<Address>), StratusError> {
    let mut deployed_contract_address = None;
    let mut execution_changes: ExecutionChanges<Complete> = ExecutionChanges::default();

    for (revm_address, mut revm_account) in revm_state {
        let address: Address = revm_address.into();
        if address.is_ignored() {
            continue;
        }

        // apply changes according to account status
        tracing::debug!(
            %address,
            status = ?revm_account.status,
            balance = %revm_account.info.balance,
            nonce = %revm_account.info.nonce,
            slots = %revm_account.storage.len(),
            "evm account"
        );

        if !(revm_account.is_created() || revm_account.is_touched()) {
            continue;
        }

        if revm_account.is_created() && revm_account.info.code.is_some() {
            deployed_contract_address = Some(address);
        }

        let storage = mem::take(&mut revm_account.storage);
        let account_modified_slots: Vec<Slot> = storage
            .into_iter()
            .filter_map(|(index, value)| match value.is_changed() {
                true => Some(Slot::new(index.into(), value.present_value.into())),
                false => None,
            })
            .collect();

        execution_changes.insert_slot_changes(address, account_modified_slots);
        if revm_account.is_changed() {
            execution_changes.insert_account_changes(address, revm_account.into());
        }
    }
    Ok((execution_changes, deployed_contract_address))
}

pub fn default_trace(tracer_type: GethDebugTracerType, tx: TransactionExecution) -> GethTrace {
    match tracer_type {
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::FourByteTracer) => FourByteFrame::default().into(),
        // HACK: Spoof empty call frame to prevent Blockscout from retrying unnecessary trace calls
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::CallTracer) => {
            let (typ, to) = match tx.evm_input.to {
                Some(_) => ("CALL".to_string(), tx.evm_input.to.map_into()),
                None => ("CREATE".to_string(), tx.result.execution.deployed_contract_address.map_into()),
            };

            CallFrame {
                from: tx.evm_input.from.into(),
                to,
                typ,
                ..Default::default()
            }
            .into()
        }
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::MuxTracer) => MuxFrame::default().into(),
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::FlatCallTracer) => FlatCallFrame::default().into(),
        _ => NoopFrame::default().into(),
    }
}

/// Enhances a GethTrace with decoded error information.
pub fn enhance_trace_with_decoded_errors(trace: &mut GethTrace) {
    match trace {
        GethTrace::CallTracer(call_frame) => {
            enhance_call_frame_errors(call_frame);
        }
        _ => {
            // Other trace types don't have call frames with errors to decode
        }
    }
}

/// Enhances a single CallFrame and recursively enhances all nested calls.
fn enhance_call_frame_errors(frame: &mut CallFrame) {
    if let Some(error) = frame.error.as_ref()
        && let Some(decoded_error) = frame.output.as_ref().and_then(|output| codegen::error_sig_opt(output))
    {
        frame.revert_reason = Some(format!("{error}: {decoded_error}"));
    }

    for nested_call in frame.calls.iter_mut() {
        enhance_call_frame_errors(nested_call);
    }
}

pub trait TxEnvExt {
    fn fill_env(&mut self, input: ExecutionInput);
}

pub trait EvmExt {
    fn fill_env(&mut self, input: ExecutionInput);
}

impl<DB: Database, I> EvmExt for GeneralRevm<DB, I> {
    fn fill_env(&mut self, input: ExecutionInput) {
        self.block.fill_env(&input);
        self.tx.fill_env(input);
    }
}

impl TxEnvExt for TxEnv {
    fn fill_env(&mut self, input: ExecutionInput) {
        self.caller = input.from.into();
        self.kind = match input.to {
            Some(contract) => TransactTo::Call(contract.into()),
            None => TransactTo::Create,
        };
        self.gas_limit = GAS_MAX_LIMIT;
        self.gas_price = 0;
        self.chain_id = input.chain_id.map_into();
        self.nonce = input.nonce.map_into().unwrap_or_default();
        self.data = input.data.into();
        self.value = input.value.into();
        self.gas_priority_fee = None;
    }
}

trait BlockEnvExt {
    fn fill_env(&mut self, input: &ExecutionInput);
}

impl BlockEnvExt for BlockEnv {
    fn fill_env(&mut self, input: &ExecutionInput) {
        self.timestamp = U256::from(*input.block_timestamp);
        self.number = U256::from(input.block_number.as_u64());
        self.basefee = 0;
    }
}

pub fn create_evm<DB: Database>(chain_id: u64, spec: SpecId, db: DB, kind: EvmKind) -> GeneralRevm<DB> {
    let ctx = Context::new(db, spec)
        .modify_cfg_chained(|cfg_env| {
            cfg_env.chain_id = chain_id;
            cfg_env.spec = spec;
            cfg_env.tx_chain_id_check = kind.is_transaction();
            cfg_env.limit_contract_initcode_size = None;
            cfg_env.disable_nonce_check = kind.is_call();
            cfg_env.max_blobs_per_tx = None;
            cfg_env.tx_gas_limit_cap = None;
            cfg_env.blob_base_fee_update_fraction = None;
            cfg_env.disable_eip3607 = kind.is_call();
            cfg_env.limit_contract_code_size = Some(usize::MAX);
            cfg_env.memory_limit = (1 << 32) - 1;
            cfg_env.disable_balance_check = false;
            cfg_env.disable_block_gas_limit = false;
            cfg_env.disable_eip3541 = false;
            cfg_env.disable_eip7623 = false;
            cfg_env.disable_base_fee = false;
            cfg_env.disable_fee_charge = false;
        })
        .modify_block_chained(|block_env: &mut BlockEnv| {
            block_env.beneficiary = Address::COINBASE.into();
        })
        .modify_tx_chained(|tx_env: &mut TxEnv| {
            tx_env.gas_priority_fee = None;
        });

    RevmEvm::new(ctx, EthInstructions::new_mainnet_with_spec(spec), EthPrecompiles::new(spec))
}
