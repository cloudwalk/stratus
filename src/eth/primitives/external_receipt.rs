use ethereum_types::U256;
use ethers_core::types::TransactionReceipt as EthersReceipt;
use serde::Deserialize;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Wei;
use crate::ext::not;
use crate::ext::OptionExt;
use crate::log_and_err;

#[derive(Debug, Clone, derive_more::Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalReceipt(#[deref] pub EthersReceipt);

impl ExternalReceipt {
    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        self.0.transaction_hash.into()
    }

    /// Returns the block number.
    pub fn block_number(&self) -> BlockNumber {
        self.0.block_number.expect("external receipt must have block number").into()
    }

    /// Returns the block hash.
    pub fn block_hash(&self) -> Hash {
        self.0.block_hash.expect("external receipt must have block hash").into()
    }

    /// Retuns the effective price the sender had to pay to execute the transaction.
    pub fn execution_cost(&self) -> Wei {
        let gas_price = self.0.effective_gas_price.map_into().unwrap_or(U256::zero());
        let gas_used = self.0.gas_used.map_into().unwrap_or(U256::zero());
        (gas_price * gas_used).into()
    }

    /// Checks if the transaction was completed with success.
    pub fn is_success(&self) -> bool {
        match self.0.status {
            Some(status) => status.as_u64() == 1,
            None => false,
        }
    }

    /// Checks if the transaction was completed with error.
    pub fn is_failure(&self) -> bool {
        not(self.is_success())
    }

    pub fn compare(&self, other: &ExternalReceipt) -> anyhow::Result<()> {
        // compare execution status
        if self.is_success() != other.is_success() {
            return log_and_err!(format!(
                "transaction status mismatch | hash={} self={:?} other={:?}",
                self.hash(),
                self.status,
                other.status
            ));
        }

        // compare logs length
        if self.logs.len() != other.logs.len() {
            tracing::trace!("self logs: {:#?}", self.logs);
            tracing::trace!("other logs: {:#?}", other.logs);
            return log_and_err!(format!(
                "logs length mismatch | hash={} execution={} receipt={}",
                self.hash(),
                self.logs.len(),
                other.logs.len()
            ));
        }

        // compare logs pairs
        for (log_index, (self_log, other_log)) in self.logs.iter().zip(&other.logs).enumerate() {
            // compare log topics length
            if self_log.topics.len() != other_log.topics.len() {
                return log_and_err!(format!(
                    "log topics length mismatch | hash={} log_index={} execution={} receipt={}",
                    self.hash(),
                    log_index,
                    self_log.topics.len(),
                    other_log.topics.len(),
                ));
            }

            // compare log topics content
            for (topic_index, (self_log_topic, other_log_topic)) in self_log.topics.iter().zip(&other_log.topics).enumerate() {
                if self_log_topic.as_ref() != other_log_topic.as_ref() {
                    return log_and_err!(format!(
                        "log topic content mismatch | hash={} log_index={} topic_index={} self={} other={:#x}",
                        self.hash(),
                        log_index,
                        topic_index,
                        self_log_topic,
                        other_log_topic,
                    ));
                }
            }

            // compare log data content
            if self_log.data.as_ref() != other_log.data.as_ref() {
                return log_and_err!(format!(
                    "log data content mismatch | hash={} log_index={} self={} other={:#x}",
                    self.hash(),
                    log_index,
                    self_log.data,
                    other_log.data,
                ));
            }
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<EthersReceipt> for ExternalReceipt {
    fn from(value: EthersReceipt) -> Self {
        ExternalReceipt(value)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl TryFrom<serde_json::Value> for ExternalReceipt {
    type Error = anyhow::Error;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        match ExternalReceipt::deserialize(&value) {
            Ok(v) => Ok(v),
            Err(e) => log_and_err!(reason = e, payload = value, "failed to convert payload value to ExternalBlock"),
        }
    }
}
