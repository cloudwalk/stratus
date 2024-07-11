use std::hash::Hash as HashTrait;

use ethers_core::types::Transaction as EthersTransaction;
use ethers_core::types::TransactionReceipt as EthersReceipt;
use itertools::Itertools;

use super::logs_bloom::LogsBloom;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionInput;
use crate::ext::OptionExt;
use crate::if_else;

/// Transaction that was executed by the EVM and added to a block.
#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct TransactionMined {
    /// Transaction input received through RPC.
    pub input: TransactionInput,

    /// Transaction EVM execution result.
    pub execution: EvmExecution,

    /// Logs added to the block.
    pub logs: Vec<LogMined>,

    /// Position of the transaction inside the block.
    pub transaction_index: Index,

    /// Block number where the transaction was mined.
    pub block_number: BlockNumber,

    /// Block hash where the transaction was mined.
    pub block_hash: Hash,
}

impl PartialOrd for TransactionMined {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TransactionMined {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.block_number, self.transaction_index).cmp(&(other.block_number, other.transaction_index))
    }
}

impl HashTrait for TransactionMined {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.input.hash.hash(state);
    }
}

impl TransactionMined {
    /// Creates a new mined transaction from an external mined transaction that was re-executed locally.
    ///
    /// TODO: this kind of conversion should be infallibe.
    pub fn from_external(tx: ExternalTransaction, receipt: ExternalReceipt, execution: EvmExecution) -> anyhow::Result<Self> {
        Ok(Self {
            input: tx.clone().try_into()?,
            execution,
            block_number: receipt.block_number(),
            block_hash: receipt.block_hash(),
            transaction_index: receipt.transaction_index.into(),
            logs: receipt.0.logs.into_iter().map(LogMined::try_from).collect::<Result<Vec<LogMined>, _>>()?,
        })
    }

    /// Check if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        self.execution.is_success()
    }

    /// Compute the LogsBloom of this transaction
    pub fn compute_bloom(&self) -> LogsBloom {
        let mut bloom = LogsBloom::default();
        for mined_log in self.logs.iter() {
            bloom.accrue(ethereum_types::BloomInput::Raw(mined_log.log.address.as_ref()));
            for topic in mined_log.topics().into_iter() {
                bloom.accrue(ethereum_types::BloomInput::Raw(topic.as_ref()));
            }
        }
        bloom
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<TransactionMined> for EthersTransaction {
    fn from(value: TransactionMined) -> Self {
        let input = value.input;
        Self {
            chain_id: input.chain_id.map_into(),
            hash: input.hash.into(),
            nonce: input.nonce.into(),
            block_hash: Some(value.block_hash.into()),
            block_number: Some(value.block_number.into()),
            transaction_index: Some(value.transaction_index.into()),
            from: input.signer.into(),
            to: input.to.map_into(),
            value: input.value.into(),
            gas_price: Some(input.gas_price.into()),
            gas: input.gas_limit.into(),
            input: input.input.into(),
            v: input.v,
            r: input.r,
            s: input.s,
            transaction_type: input.tx_type,
            ..Default::default()
        }
    }
}

impl From<TransactionMined> for EthersReceipt {
    fn from(value: TransactionMined) -> Self {
        let logs_bloom = value.compute_bloom().0;
        Self {
            // receipt specific
            status: Some(if_else!(value.is_success(), 1, 0).into()),
            contract_address: value.execution.contract_address().map_into(),
            gas_used: Some(value.execution.gas.into()),

            // transaction
            transaction_hash: value.input.hash.into(),
            from: value.input.signer.into(),
            to: value.input.to.map_into(),

            // block
            block_hash: Some(value.block_hash.into()),
            block_number: Some(value.block_number.into()),
            transaction_index: value.transaction_index.into(),

            // logs
            logs: value.logs.into_iter().map_into().collect(),
            logs_bloom, // TODO: save this to the database instead of computing it every time (could also be useful for eth_getLogs)

            // TODO: there are more fields to populate here
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use fake::Fake;
    use fake::Faker;
    use hex_literal::hex;
    use rand::Rng;

    use super::*;
    use crate::eth::primitives::Log;

    fn create_tx(transaction_index: u64, block_number: u64) -> TransactionMined {
        TransactionMined {
            input: Faker.fake(),
            execution: Faker.fake(),
            logs: vec![],
            transaction_index: transaction_index.into(),
            block_number: block_number.into(),
            block_hash: Hash::default(),
        }
    }

    #[test]
    fn sort_transactions() {
        let mut rng = rand::thread_rng();
        let v = (0..1000)
            .map(|_| create_tx(rng.gen_range(0..100), rng.gen_range(0..100)))
            .sorted()
            .map(|tx| (tx.block_number.as_u64(), tx.transaction_index.inner_value()))
            .collect_vec();
        for pair in v {
            format!("{:?}", pair);
        }
    }

    #[test]
    fn compute_bloom() {
        let mut tx = create_tx(0, 0);
        tx.logs.push(LogMined {
            transaction_hash: tx.input.hash,
            transaction_index: tx.transaction_index,
            log_index: 0.into(),
            block_number: tx.block_number,
            block_hash: tx.block_hash,
            log: Log {
                address: hex!("c6d1efd908ef6b69da0749600f553923c465c812").into(),
                topic0: Some(hex!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef").into()),
                topic1: Some(hex!("000000000000000000000000d1ff9b395856e5a6810f626eca09d61d34fce3b8").into()),
                topic2: Some(hex!("000000000000000000000000081d2c5b26db6f6e0944e4725b3b61b26e25dd8a").into()),
                topic3: None,
                data: hex!("0000000000000000000000000000000000000000000000000000000005f5e100").as_ref().into(),
            },
        });
        tx.logs.push(LogMined {
            transaction_hash: tx.input.hash,
            transaction_index: tx.transaction_index,
            log_index: 1.into(),
            block_number: tx.block_number,
            block_hash: tx.block_hash,
            log: Log {
                address: hex!("b1f571b3254c99a0a562124738f0193de2b2b2a9").into(),
                topic0: Some(hex!("8d995e7fbf7a5ef41cee9e6936368925d88e07af89306bb78a698551562e683c").into()),
                topic1: Some(hex!("000000000000000000000000081d2c5b26db6f6e0944e4725b3b61b26e25dd8a").into()),
                topic2: None,
                topic3: None,
                data: hex!(
                    "0000000000000000000000000000000000000000000000000000000000004dca00000000\
                0000000000000000000000000000000000000000000000030c9281f0"
                )
                .as_ref()
                .into(),
            },
        });
        let expected: LogsBloom = hex!(
            "000000000400000000000000000000000000000000000000000000000000\
        00000000000000000000000000000000000000080000000000000000000000000000000000000000000000000008\
        00008400202000000000002000000000000000000000000000000010000000000000000000000000040000000000\
        00100000000000000000000000000000000000000000000000000000000000000000000000001000000000000100\
        00000000000000000000000000000000000000000000080000000002000000000000000000000000000000002000\
        000000440000000000000000000000000000000000000000000000000000000000000000000000000000"
        )
        .into();

        assert_eq!(tx.compute_bloom(), expected);
    }
}
