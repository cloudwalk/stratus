#[cfg(test)]
use alloy_eips::eip4895::Withdrawals;
#[cfg(test)]
use alloy_primitives::B64;
#[cfg(test)]
use alloy_primitives::B256;
#[cfg(test)]
use alloy_primitives::Bloom;
#[cfg(test)]
use alloy_primitives::Bytes;
#[cfg(test)]
use alloy_primitives::U256;
use alloy_rpc_types_eth::BlockTransactions;
use anyhow::bail;
#[cfg(test)]
use fake::Dummy;
#[cfg(test)]
use fake::Fake;
#[cfg(test)]
use fake::Faker;
use serde::Deserialize;

use crate::alias::AlloyBlockExternalTransaction;
use crate::alias::JsonValue;
use crate::eth::types::Address;
use crate::eth::types::Block;
use crate::eth::types::BlockNumber;
#[cfg(test)]
use crate::eth::types::ExternalTransaction;
use crate::eth::types::Hash;
use crate::eth::types::UnixTime;
use crate::log_and_err;

#[derive(Debug, Clone, PartialEq, derive_more::Deref, derive_more::DerefMut, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ExternalBlock(#[deref] pub AlloyBlockExternalTransaction);

impl ExternalBlock {
    /// Returns the block hash.
    #[allow(clippy::expect_used)]
    pub fn hash(&self) -> Hash {
        Hash::from(self.0.header.hash)
    }

    /// Returns the block number.
    #[allow(clippy::expect_used)]
    pub fn number(&self) -> BlockNumber {
        BlockNumber::from(self.0.header.inner.number)
    }

    /// Returns the block timestamp.
    pub fn timestamp(&self) -> UnixTime {
        self.0.header.inner.timestamp.into()
    }

    /// Returns the block author.
    pub fn author(&self) -> Address {
        self.0.header.inner.beneficiary.into()
    }

    /// Returns the number of full transactions in the block.
    pub fn full_transactions_len(&self) -> anyhow::Result<usize> {
        let BlockTransactions::Full(transactions) = &self.0.transactions else {
            bail!("expected full transactions, got hashes or uncle");
        };

        Ok(transactions.len())
    }

    /// Appends full transactions from another page of the same block.
    pub fn extend_full_transactions_from(&mut self, other: Self) -> anyhow::Result<()> {
        if self.hash() != other.hash() {
            bail!(
                "cannot extend external block transactions from block {} into block {}",
                other.hash(),
                self.hash()
            );
        }

        let BlockTransactions::Full(other_transactions) = other.0.transactions else {
            bail!("expected full transactions, got hashes or uncle");
        };
        let BlockTransactions::Full(transactions) = &mut self.0.transactions else {
            bail!("expected full transactions, got hashes or uncle");
        };

        transactions.extend(other_transactions);
        Ok(())
    }
}

impl PartialEq<Block> for ExternalBlock {
    fn eq(&self, other: &Block) -> bool {
        self.number() == other.number() && self.timestamp() == other.header.timestamp && self.hash() == other.header.hash
    }
}

#[cfg(test)]
impl Dummy<Faker> for ExternalBlock {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(faker: &Faker, rng: &mut R) -> Self {
        let mut addr_bytes = [0u8; 20];
        let mut hash_bytes = [0u8; 32];
        let mut nonce_bytes = [0u8; 8];
        rng.fill_bytes(&mut addr_bytes);
        rng.fill_bytes(&mut hash_bytes);
        rng.fill_bytes(&mut nonce_bytes);

        let transaction: ExternalTransaction = faker.fake_with_rng(rng);

        let block = alloy_rpc_types_eth::Block {
            header: alloy_rpc_types_eth::Header {
                hash: B256::from_slice(&hash_bytes),
                inner: alloy_consensus::Header {
                    parent_hash: B256::from_slice(&hash_bytes),
                    ommers_hash: B256::from_slice(&hash_bytes),
                    beneficiary: alloy_primitives::Address::from_slice(&addr_bytes),
                    state_root: B256::from_slice(&hash_bytes),
                    transactions_root: B256::from_slice(&hash_bytes),
                    receipts_root: B256::from_slice(&hash_bytes),
                    withdrawals_root: Some(B256::from_slice(&hash_bytes)),
                    number: rng.next_u64(),
                    gas_used: rng.next_u64(),
                    gas_limit: rng.next_u64(),
                    extra_data: Bytes::default(),
                    logs_bloom: Bloom::default(),
                    timestamp: rng.next_u64(),
                    difficulty: U256::from(rng.next_u64()),
                    mix_hash: B256::from_slice(&hash_bytes),
                    nonce: B64::from_slice(&nonce_bytes),
                    base_fee_per_gas: Some(rng.next_u64()),
                    blob_gas_used: None,
                    excess_blob_gas: None,
                    parent_beacon_block_root: None,
                    requests_hash: None,
                    block_access_list_hash: None,
                    slot_number: None,
                },
                total_difficulty: Some(U256::from(rng.next_u64())),
                size: Some(U256::from(rng.next_u64())),
            },
            uncles: vec![B256::from_slice(&hash_bytes)],
            transactions: alloy_rpc_types_eth::BlockTransactions::Full(vec![transaction]),
            withdrawals: Some(Withdrawals::default()),
        };

        ExternalBlock(block)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl TryFrom<JsonValue> for ExternalBlock {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        match ExternalBlock::deserialize(&value) {
            Ok(v) => Ok(v),
            Err(e) => log_and_err!(reason = e, payload = value, "failed to convert payload value to ExternalBlock"),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::B256;
    use alloy_rpc_types_eth::BlockTransactions;
    use fake::Fake;
    use fake::Faker;

    use super::ExternalBlock;
    use crate::eth::types::ExternalTransaction;

    // Builds an ExternalBlock with a fixed hash and `count` random full transactions.
    fn block_with_txs(count: usize) -> ExternalBlock {
        let mut block: ExternalBlock = Faker.fake();
        block.0.header.hash = fixed_hash();
        let txs: Vec<ExternalTransaction> = std::iter::repeat_with(|| Faker.fake()).take(count).collect();
        block.0.transactions = BlockTransactions::Full(txs);
        block
    }

    fn fixed_hash() -> B256 {
        B256::from_slice(&[0xAA; 32])
    }

    fn as_full(block: &ExternalBlock) -> &Vec<ExternalTransaction> {
        let BlockTransactions::Full(txs) = &block.0.transactions else {
            unreachable!("expected full transactions");
        };
        txs
    }

    #[test]
    fn full_transactions_len_with_full() {
        let block = block_with_txs(3);
        assert_eq!(block.full_transactions_len().expect("full transactions"), 3);
    }

    #[test]
    fn full_transactions_len_with_hashes() {
        let mut block = block_with_txs(0);
        block.0.transactions = BlockTransactions::Hashes(vec![fixed_hash()]);
        assert!(block.full_transactions_len().is_err());
    }

    #[test]
    fn full_transactions_len_with_uncle() {
        let mut block = block_with_txs(0);
        block.0.transactions = BlockTransactions::Uncle;
        assert!(block.full_transactions_len().is_err());
    }

    #[test]
    fn extend_full_transactions_from_same_hash_merges() {
        let mut target = block_with_txs(2);
        let other = block_with_txs(1);

        target.extend_full_transactions_from(other).expect("same hash merges");

        assert_eq!(target.full_transactions_len().expect("full transactions"), 3);
    }

    #[test]
    fn extend_full_transactions_from_different_hash_errors() {
        let mut target = block_with_txs(1);
        let mut other = block_with_txs(1);
        other.0.header.hash = B256::from_slice(&[0xBB; 32]);

        assert!(target.extend_full_transactions_from(other).is_err());
    }

    #[test]
    fn extend_full_transactions_from_non_full_source_errors() {
        let mut target = block_with_txs(1);
        let mut other = block_with_txs(0);
        other.0.transactions = BlockTransactions::Hashes(vec![fixed_hash()]);

        assert!(target.extend_full_transactions_from(other).is_err());
    }

    #[test]
    fn extend_full_transactions_into_non_full_target_errors() {
        let mut target = block_with_txs(0);
        target.0.transactions = BlockTransactions::Hashes(vec![fixed_hash()]);
        let other = block_with_txs(1);

        assert!(target.extend_full_transactions_from(other).is_err());
    }

    #[test]
    fn extend_full_transactions_preserves_order() {
        let mut target = block_with_txs(1);
        let original = as_full(&target).clone();
        let other = block_with_txs(2);
        let other_txs = as_full(&other).clone();

        target.extend_full_transactions_from(other).expect("same hash merges");

        let merged = as_full(&target);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0], original[0]);
        assert_eq!(merged[1], other_txs[0]);
        assert_eq!(merged[2], other_txs[1]);
    }
}
