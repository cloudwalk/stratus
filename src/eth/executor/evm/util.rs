use alloy_primitives::U256;
use alloy_rpc_types_trace::geth::CallFrame;
use alloy_rpc_types_trace::geth::FourByteFrame;
use alloy_rpc_types_trace::geth::GethDebugBuiltInTracerType;
use alloy_rpc_types_trace::geth::GethDebugTracerType;
use alloy_rpc_types_trace::geth::GethTrace;
use alloy_rpc_types_trace::geth::NoopFrame;
use alloy_rpc_types_trace::geth::call::FlatCallFrame;
use alloy_rpc_types_trace::geth::mux::MuxFrame;
use revm::Context;
use revm::Database;
use revm::context::BlockEnv;
use revm::context::Evm as RevmEvm;
use revm::context::TransactTo;
use revm::context::TxEnv;
use revm::handler::EthPrecompiles;
use revm::handler::instructions::EthInstructions;
use revm::primitives::hardfork::SpecId;

use crate::eth::codegen;
use crate::eth::executor::EvmKind;
use crate::eth::executor::TransactionExecution;
use crate::eth::executor::TransactionExecutionInput;
use crate::eth::executor::evm::GeneralRevm;
use crate::eth::executor::evm::types::GAS_MAX_LIMIT;
use crate::eth::types::Address;
use crate::ext::OptionExt;

pub fn default_trace(tracer_type: GethDebugTracerType, tx: TransactionExecution) -> GethTrace {
    match tracer_type {
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::FourByteTracer) => FourByteFrame::default().into(),
        // HACK: Spoof empty call frame to prevent Blockscout from retrying unnecessary trace calls
        GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::CallTracer) => {
            let (typ, to) = match tx.input.to {
                Some(_) => ("CALL".to_string(), tx.input.to.map_into()),
                None => ("CREATE".to_string(), tx.output.deployed_contract_address.map_into()),
            };

            CallFrame {
                from: tx.input.from.into(),
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
    fn fill_env(&mut self, input: TransactionExecutionInput);
}

pub trait EvmExt {
    fn fill_env(&mut self, input: TransactionExecutionInput);
}

impl<DB: Database, I> EvmExt for GeneralRevm<DB, I> {
    fn fill_env(&mut self, input: TransactionExecutionInput) {
        self.block.fill_env(&input);
        self.tx.fill_env(input);
    }
}

impl TxEnvExt for TxEnv {
    fn fill_env(&mut self, input: TransactionExecutionInput) {
        self.caller = input.from.into();
        self.kind = match input.to {
            Some(contract) => TransactTo::Call(contract.into()),
            None => TransactTo::Create,
        };
        self.gas_limit = GAS_MAX_LIMIT;
        self.gas_price = 0;
        self.chain_id = input.chain_id.map_into();
        self.nonce = input.nonce.into();
        self.data = input.data.into();
        self.value = input.value.into();
        self.gas_priority_fee = None;
    }
}

trait BlockEnvExt {
    fn fill_env(&mut self, input: &TransactionExecutionInput);
}

impl BlockEnvExt for BlockEnv {
    fn fill_env(&mut self, input: &TransactionExecutionInput) {
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
