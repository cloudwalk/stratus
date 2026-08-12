use alloy_primitives::U256;
use alloy_rpc_types_trace::geth::GethDebugTracingOptions;
use anyhow::anyhow;
use display_json::DebugAsJson;
use revm::Context;
use revm::Database;
use revm::Journal;
use revm::context::BlockEnv;
use revm::context::CfgEnv;
use revm::context::Evm as RevmEvm;
use revm::context::TransactTo;
use revm::context::TxEnv;
use revm::handler::EthFrame;
use revm::handler::EthPrecompiles;
use revm::handler::instructions::EthInstructions;
use revm::interpreter::interpreter::EthInterpreter;

use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::UnixTime;
use crate::eth::primitives::Wei;
use crate::eth::storage::ExecutionKind;
use crate::eth::storage::TxCount;
use crate::ext::OptionExt;

/// Maximum gas limit allowed for a transaction. Prevents a transaction from consuming too many resources.
#[cfg(feature = "dev")]
pub const GAS_MAX_LIMIT: u64 = 1_000_000_000;
#[cfg(not(feature = "dev"))]
pub const GAS_MAX_LIMIT: u64 = 100_000_000;

pub type ContextWithDB<DB> = Context<BlockEnv, TxEnv, CfgEnv, DB, Journal<DB>>;
pub type GeneralRevm<DB, I = ()> = RevmEvm<ContextWithDB<DB>, I, EthInstructions<EthInterpreter, ContextWithDB<DB>>, EthPrecompiles, EthFrame>;

/// EVM input data. Usually derived from a transaction or call.
#[derive(DebugAsJson, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct TransactionExecutionInput {
    /// Operation party address.
    ///
    /// It can be:
    /// * Transaction signer when executing an `eth_sendRawTransaction`.
    /// * Placeholder when performing an `eth_call`
    /// * Not specified when performing an `eth_call`
    pub from: Address,

    /// Operation counterparty address.
    ///
    /// It can be:
    /// * Contract address when performing a function call.
    /// * Destination account address when transfering funds.
    /// * Not specified when deploying a contract.
    pub to: Option<Address>,

    /// Transfered amount from party to counterparty.
    ///
    /// Present only in native token transfers. When calling a contract function, the value is usually zero.
    pub value: Wei,

    /// Operation data.
    ///
    /// It can be:
    /// * Function ID and parameters when performing a contract function call.
    /// * Not specified when transfering funds.
    /// * Contract bytecode when deploying a contract.
    pub data: Bytes,

    /// Operation party nonce.
    ///
    /// It can be:
    /// * Required when executing an `eth_sendRawTransaction`.
    /// * Not specified when performing an `eth_call`.
    pub nonce: Nonce,

    /// Max gas consumption allowed for the transaction.
    pub gas_limit: Gas,

    /// Gas price paid by each unit of gas consumed by the transaction.
    pub gas_price: u128,

    /// Number of the block where the transaction will be or was included.
    pub block_number: BlockNumber,

    /// Timestamp of the block where the transaction will be or was included.
    pub block_timestamp: UnixTime,

    /// ID of the blockchain where the transaction will be or was included.
    ///
    /// If not specified, it will not be validated.
    pub chain_id: Option<ChainId>,

    pub kind: ExecutionKind,
}

impl TransactionExecutionInput {
    /// Creates from a transaction that was sent to Stratus with `eth_sendRawTransaction` or during Importing.
    pub fn from_eth_transaction(input: &TransactionInput, block_number: BlockNumber, block_timestamp: UnixTime) -> Self {
        Self {
            from: input.signer(),
            to: input.execution_info.to,
            value: input.execution_info.value,
            data: input.execution_info.input.clone(),
            gas_limit: input.execution_info.gas_limit,
            gas_price: input.execution_info.gas_price,
            nonce: input.execution_info.nonce,
            block_number,
            block_timestamp,
            chain_id: input.execution_info.chain_id,
            kind: ExecutionKind::Transaction,
        }
    }
}

impl PartialEq<&PendingBlockHeader> for TransactionExecutionInput {
    fn eq(&self, other: &&PendingBlockHeader) -> bool {
        self.block_number == other.number && self.block_timestamp == *other.timestamp
    }
}

pub struct InspectorInput {
    pub tx_hash: Hash,
    pub opts: GethDebugTracingOptions,
    pub trace_unsuccessful_only: bool,
}

#[derive(Clone, Copy)]
pub enum EvmKind {
    Transaction,
    CallPast,
    CallPresent,
    Inspect,
}

impl EvmKind {
    pub fn is_call(&self) -> bool {
        match self {
            EvmKind::Transaction => false,
            EvmKind::CallPast | EvmKind::CallPresent | EvmKind::Inspect => true,
        }
    }

    pub fn is_transaction(&self) -> bool {
        !self.is_call()
    }
}

/// EVM input data. Usually derived from a transaction or call.
#[derive(DebugAsJson, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct CallExecutionInput {
    /// Operation party address.
    ///
    /// It can be:
    /// * Transaction signer when executing an `eth_sendRawTransaction`.
    /// * Placeholder when performing an `eth_call`
    /// * Not specified when performing an `eth_call`
    pub from: Address,

    /// Operation counterparty address.
    ///
    /// It can be:
    /// * Contract address when performing a function call.
    /// * Destination account address when transfering funds.
    /// * Not specified when deploying a contract.
    pub to: Option<Address>,

    /// Transfered amount from party to counterparty.
    ///
    /// Present only in native token transfers. When calling a contract function, the value is usually zero.
    pub value: Wei,

    /// Operation data.
    ///
    /// It can be:
    /// * Function ID and parameters when performing a contract function call.
    /// * Not specified when transfering funds.
    /// * Contract bytecode when deploying a contract.
    pub data: Bytes,

    /// Number of the block where the transaction will be or was included.
    pub block_number: BlockNumber,

    /// Timestamp of the block where the transaction will be or was included.
    pub block_timestamp: UnixTime,

    pub kind: ExecutionKind,
}

impl CallExecutionInput {
    /// Creates from a call that was sent directly to Stratus with `eth_call` or `eth_estimateGas` for a pending block.
    pub fn from_pending_block(input: CallInput, pending_header: PendingBlockHeader, tx_count: TxCount) -> Self {
        Self {
            from: input.from.unwrap_or(Address::ZERO),
            to: input.to.map_into(),
            value: input.value,
            data: input.data,
            block_number: pending_header.number,
            block_timestamp: *pending_header.timestamp,
            kind: ExecutionKind::CallPending(pending_header.number, tx_count),
        }
    }

    /// Creates from a call that was sent directly to Stratus with `eth_call` or `eth_estimateGas` for a mined block.
    pub fn try_from_mined_block(input: CallInput, block: Block, point_in_time: PointInTime) -> anyhow::Result<Self, StratusError> {
        let kind = match point_in_time {
            PointInTime::Latest => ExecutionKind::CallLatest(block.number()),
            PointInTime::Past(number) => ExecutionKind::CallPast(number),
            PointInTime::Pending => return Err(anyhow!("call execution cannot be created on mined block with PointInTime::Pending").into()),
        };
        Ok(Self {
            from: input.from.unwrap_or(Address::ZERO),
            to: input.to.map_into(),
            value: input.value,
            data: input.data,
            block_number: block.number(),
            block_timestamp: block.header.timestamp,
            kind,
        })
    }
}

pub trait EvmInput: Default + Clone {
    fn kind(&self) -> ExecutionKind;

    fn fill_tx_env<DB: Database, I>(self, evm: &mut GeneralRevm<DB, I>);

    fn fill_block_env<DB: Database, I>(&self, evm: &mut GeneralRevm<DB, I>);

    fn fill_env<DB: Database, I>(self, evm: &mut GeneralRevm<DB, I>) {
        self.fill_block_env(evm);
        self.fill_tx_env(evm);
    }
}

impl EvmInput for CallExecutionInput {
    fn fill_block_env<DB: Database, I>(&self, evm: &mut GeneralRevm<DB, I>) {
        evm.block.timestamp = U256::from(*self.block_timestamp);
        evm.block.number = U256::from(self.block_number.as_u64());
        evm.block.basefee = 0;
    }

    fn fill_tx_env<DB: Database, I>(self, evm: &mut GeneralRevm<DB, I>) {
        evm.tx.caller = self.from.into();
        evm.tx.kind = match self.to {
            Some(contract) => TransactTo::Call(contract.into()),
            None => TransactTo::Create,
        };
        evm.tx.gas_limit = GAS_MAX_LIMIT;
        evm.tx.gas_price = 0;
        evm.tx.chain_id = None;
        evm.tx.nonce = 0;
        evm.tx.data = self.data.into();
        evm.tx.value = self.value.into();
        evm.tx.gas_priority_fee = None;
    }

    fn kind(&self) -> ExecutionKind {
        self.kind
    }
}

impl EvmInput for TransactionExecutionInput {
    fn fill_block_env<DB: Database, I>(&self, evm: &mut GeneralRevm<DB, I>) {
        evm.block.timestamp = U256::from(*self.block_timestamp);
        evm.block.number = U256::from(self.block_number.as_u64());
        evm.block.basefee = 0;
    }

    fn fill_tx_env<DB: Database, I>(self, evm: &mut GeneralRevm<DB, I>) {
        evm.tx.caller = self.from.into();
        evm.tx.kind = match self.to {
            Some(contract) => TransactTo::Call(contract.into()),
            None => TransactTo::Create,
        };
        evm.tx.gas_limit = GAS_MAX_LIMIT;
        evm.tx.gas_price = 0;
        evm.tx.chain_id = self.chain_id.map_into();
        evm.tx.nonce = self.nonce.into();
        evm.tx.data = self.data.into();
        evm.tx.value = self.value.into();
        evm.tx.gas_priority_fee = None;
    }

    fn kind(&self) -> ExecutionKind {
        self.kind
    }
}
