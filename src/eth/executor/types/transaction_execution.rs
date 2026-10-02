use alloy_consensus::Eip658Value;
use alloy_consensus::Receipt;
use alloy_consensus::ReceiptEnvelope;
use alloy_consensus::ReceiptWithBloom;
use display_json::DebugAsJson;

use crate::alias::AlloyAddress;
use crate::alias::AlloyLog;
use crate::alias::AlloyLogData;
use crate::alias::AlloyLogPrimitive;
use crate::alias::AlloyReceipt;
use crate::alias::AlloyTransaction;
use crate::eth::executor::TransactionExecutionInput;
use crate::eth::executor::TransactionExecutionResult;
use crate::eth::types::Log;
use crate::eth::types::LogsBloom;
use crate::eth::types::MinedData;
use crate::eth::types::Signature;
use crate::eth::types::TransactionInfo;
use crate::eth::types::TransactionInput;
use crate::ext::OptionExt;
use crate::ext::RuintExt;

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct TransactionExecution {
    pub info: TransactionInfo,
    pub signature: Signature,
    pub input: TransactionExecutionInput,
    pub output: TransactionExecutionResult,
}

impl TransactionExecution {
    pub fn create_alloy_logs(&self) -> Vec<AlloyLog> {
        self.logs()
            .iter()
            .map(|log| AlloyLog {
                inner: AlloyLogPrimitive {
                    address: log.address.into(),
                    data: AlloyLogData::new_unchecked(log.topics_non_empty().into_iter().map(Into::into).collect(), log.data.clone().into()),
                },
                block_hash: None,
                block_number: Some(self.input.block_number.as_u64()),
                block_timestamp: Some(*self.input.block_timestamp),
                transaction_hash: Some(self.info.hash.into()),
                transaction_index: None,
                log_index: None,
                removed: false,
            })
            .collect()
    }

    /// Computes the bloom filter from execution logs.
    fn compute_bloom(&self) -> LogsBloom {
        let mut bloom = LogsBloom::default();
        for log in self.output.logs.iter() {
            bloom.accrue_log(log);
        }
        bloom
    }

    pub fn logs(&self) -> &Vec<Log> {
        &self.output.logs
    }

    /// Builds an [`AlloyReceipt`] from this execution, optionally enriching it
    /// with mined-position data (`transaction_index`, `block_hash`).
    ///
    /// Shared between [`TransactionExecution`] (no mined data) and
    /// [`crate::eth::types::TransactionMined`] (with mined data).
    pub fn to_alloy_receipt(&self, alloy_logs: Vec<AlloyLog>, mined_data: Option<MinedData>) -> AlloyReceipt {
        let receipt = Receipt {
            status: Eip658Value::Eip658(self.output.result.is_success()),
            cumulative_gas_used: self.output.gas_used.into(), // TODO: implement cumulative gas used correctly
            logs: alloy_logs,
        };

        let receipt_with_bloom = ReceiptWithBloom {
            receipt,
            logs_bloom: self.compute_bloom().into(),
        };

        let inner = match self.info.tx_type.map(|tx| tx.as_u64()) {
            Some(1u64) => ReceiptEnvelope::Eip2930(receipt_with_bloom),
            Some(2u64) => ReceiptEnvelope::Eip1559(receipt_with_bloom),
            Some(3u64) => ReceiptEnvelope::Eip4844(receipt_with_bloom),
            Some(4u64) => ReceiptEnvelope::Eip7702(receipt_with_bloom),
            _ => ReceiptEnvelope::Legacy(receipt_with_bloom),
        };

        AlloyReceipt {
            inner,
            transaction_hash: self.info.hash.into(),
            transaction_index: mined_data.map(|data| data.index.into()),
            block_hash: mined_data.map(|data| data.block_hash.into()),
            block_number: Some(self.input.block_number.as_u64()),
            gas_used: self.output.gas_used.into(),
            effective_gas_price: self.input.gas_price,
            blob_gas_used: None,
            blob_gas_price: None,
            from: self.input.from.into(),
            to: self.input.to.map_into(),
            contract_address: self.receipt_contract_address(),
        }
    }

    fn receipt_contract_address(&self) -> Option<AlloyAddress> {
        let is_successful_deployment = self.input.to.is_none() && self.output.result.is_success();
        is_successful_deployment.then(|| AlloyAddress::from(self.input.from).create(self.input.nonce.as_u64()))
    }
}

impl From<TransactionExecution> for AlloyTransaction {
    fn from(value: TransactionExecution) -> Self {
        let tx_input: TransactionInput = value.into();
        tx_input.into()
    }
}

impl From<TransactionExecution> for TransactionInput {
    fn from(value: TransactionExecution) -> Self {
        Self {
            transaction_info: value.info,
            execution_info: value.input.into(),
            signature: value.signature,
        }
    }
}

impl From<TransactionExecution> for AlloyReceipt {
    fn from(value: TransactionExecution) -> Self {
        let alloy_logs = value.create_alloy_logs();
        value.to_alloy_receipt(alloy_logs, None)
    }
}

#[cfg(test)]
mod tests {
    use fake::Fake;
    use fake::Faker;
    use hex_literal::hex;

    use super::*;
    use crate::eth::executor::ExecutionResult;
    use crate::eth::types::Address;
    use crate::eth::types::Nonce;

    const SENDER: Address = Address(alloy_primitives::FixedBytes(hex!("6ac7ea33f8831ea9dcc53393aaa88b25a785dbf0")));
    const SENDER_NONCE_0_CREATE_ADDRESS: AlloyAddress = AlloyAddress::new(hex!("cd234a471b72ba2f1ccf0a70fcaba648a5eecd8d"));

    fn execution(to: Option<Address>, result: ExecutionResult) -> TransactionExecution {
        let mut execution: TransactionExecution = Faker.fake();
        execution.input.from = SENDER;
        execution.input.nonce = Nonce::ZERO;
        execution.input.to = to;
        execution.output.result = result;
        execution.output.deployed_contract_address = Some(Faker.fake());
        execution
    }

    #[test]
    fn test_receipt_contract_address_of_deployment_is_derived_from_sender_and_nonce() {
        let receipt = AlloyReceipt::from(execution(None, ExecutionResult::Success));
        assert_eq!(receipt.contract_address, Some(SENDER_NONCE_0_CREATE_ADDRESS));
    }

    #[test]
    fn test_receipt_contract_address_of_call_to_factory_is_none() {
        let receipt = AlloyReceipt::from(execution(Some(Faker.fake()), ExecutionResult::Success));
        assert_eq!(receipt.contract_address, None);
    }

    #[test]
    fn test_receipt_contract_address_of_failed_deployment_is_none() {
        let reverted = ExecutionResult::Reverted { reason: Faker.fake() };
        assert_eq!(AlloyReceipt::from(execution(None, reverted)).contract_address, None);

        let halted = ExecutionResult::Halted { reason: Faker.fake() };
        assert_eq!(AlloyReceipt::from(execution(None, halted)).contract_address, None);
    }
}
