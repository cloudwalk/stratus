use display_json::DebugAsJson;

use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::UnixTime;
use crate::eth::primitives::Wei;
use crate::eth::storage::StoragePointInTime;
use crate::ext::not;
use crate::ext::OptionExt;
use crate::if_else;
use crate::log_and_err;

/// EVM input data. Usually derived from a transaction or call.
#[derive(DebugAsJson, Clone, Default, serde::Serialize)]
pub struct EvmInput {
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
    pub nonce: Option<Nonce>,

    /// Max gas consumption allowed for the transaction.
    pub gas_limit: Gas,

    /// Gas price paid by each unit of gas consumed by the transaction.
    pub gas_price: Wei,

    /// Number of the block where the transaction will be or was included.
    pub block_number: BlockNumber,

    /// Timestamp of the block where the transaction will be or was included.
    pub block_timestamp: UnixTime,

    /// Point-in-time from where accounts and slots will be read.
    pub point_in_time: StoragePointInTime,

    /// ID of the blockchain where the transaction will be or was included.
    ///
    /// If not specified, it will not be validated.
    pub chain_id: Option<ChainId>,
}

impl EvmInput {
    /// Creates from a transaction that was sent directly to Stratus with `eth_sendRawTransaction`.
    pub fn from_eth_transaction(input: TransactionInput, pending_header: PendingBlockHeader) -> Self {
        Self {
            from: input.signer,
            to: input.to,
            value: input.value,
            data: input.input,
            gas_limit: Gas::MAX,
            gas_price: Wei::ZERO,
            nonce: Some(input.nonce),
            block_number: pending_header.number,
            block_timestamp: *pending_header.timestamp,
            point_in_time: StoragePointInTime::Pending,
            chain_id: input.chain_id,
        }
    }

    /// Creates from a call that was sent directly to Stratus with `eth_call` or `eth_estimateGas`.
    pub fn from_eth_call(
        input: CallInput,
        point_in_time: StoragePointInTime,
        pending_header: PendingBlockHeader,
        mined_block: Option<Block>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            from: input.from.unwrap_or(Address::ZERO),
            to: input.to.map_into(),
            value: input.value,
            data: input.data,
            gas_limit: Gas::MAX,
            gas_price: Wei::ZERO,
            nonce: None,
            block_number: match point_in_time {
                StoragePointInTime::Mined | StoragePointInTime::Pending => pending_header.number,
                StoragePointInTime::MinedPast(number) => number,
            },
            block_timestamp: match point_in_time {
                StoragePointInTime::Mined | StoragePointInTime::Pending => *pending_header.timestamp,
                StoragePointInTime::MinedPast(_) => match mined_block {
                    Some(block) => block.header.timestamp,
                    None => return log_and_err!("failed to create EvmInput because cannot determine mined block timestamp"),
                },
            },
            point_in_time,
            chain_id: None,
        })
    }

    /// Creates a transaction that was executed in an external blockchain and imported to Stratus.
    ///
    /// Successful external transactions executes with max gas and zero gas price to ensure we will have the same execution result.
    pub fn from_external(tx: &ExternalTransaction, receipt: &ExternalReceipt, block_number: BlockNumber, block_timestamp: UnixTime) -> anyhow::Result<Self> {
        Ok(Self {
            from: tx.0.from.into(),
            to: tx.0.to.map_into(),
            value: tx.0.value.into(),
            data: tx.0.input.clone().into(),
            nonce: Some(tx.0.nonce.try_into()?),
            gas_limit: if_else!(receipt.is_success(), Gas::MAX, tx.0.gas.try_into()?),
            gas_price: if_else!(receipt.is_success(), Wei::ZERO, tx.0.gas_price.map_into().unwrap_or(Wei::ZERO)),
            point_in_time: StoragePointInTime::Pending,
            block_number,
            block_timestamp,
            chain_id: match tx.0.chain_id {
                Some(chain_id) => Some(chain_id.try_into()?),
                None => None,
            },
        })
    }

    /// Checks if the input is a contract call.
    ///
    /// It is when there is a `to` address and the `data` field is also populated.
    pub fn is_contract_call(&self) -> bool {
        self.to.is_some() && not(self.data.is_empty())
    }
}
