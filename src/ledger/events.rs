use chrono::DateTime;
use chrono::Utc;
use display_json::DebugAsJson;
use ethereum_types::U256;
use serde::Serialize;
use serde_with::serde_as;
use serde_with::DisplayFromStr;
use uuid::Uuid;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;

/// Represents token transfers (debits and credits) associated with a specific Ethereum account within a single transaction.
///
/// The `primary_account_address` field identifies the primary account involved in these transfers.
///
/// An event will be generated for each account involved in a transaction, meaning if a transaction is not the primary in this event,
/// in another event it will treated as the primary and credit and debit operations adjusted accordingly.
///
/// A single event can contain multiple token transfers (e.g., a customer is debited for a card payment but receives a credit as cashback)
#[derive(DebugAsJson, Serialize)]
pub struct AccountTransfersEvent {
    /// ID of the event publication.
    ///
    /// If the event is republished, a new ID will be generated for the publication.
    ///
    /// Format: UUID v7 (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx)
    pub publication_id: Uuid,

    /// Datetime of the event publication.
    ///
    /// Format: ISO 8601
    pub publication_datetime: DateTime<Utc>,

    /// Idempotency key of the event payload (TBD: `primary_account_address` + `transaction_hash`).
    ///
    /// It is unique for each distinct event, but consistent across retries or republications of the same event payload.
    ///
    /// Format: String (TBD)
    pub idempotency_key: String,

    /// Address of the account that is part of all transfers.
    ///
    /// Format: Prefixed account address - 20 bytes - 0x1234567890123456789012345678901234567890
    pub primary_account_address: Address,

    /// Hash of the Ethereum transaction that originated transfers.
    ///
    /// Format: Prefixed transaction hash - 32 bytes - 0x1234567890123456789012345678901234567890123456789012345678901234
    pub transaction_hash: Hash,

    /// Datetime of the Ethereum transaction that originated transfers.
    ///
    /// Format: ISO 8601
    pub transaction_datetime: DateTime<Utc>,

    /// Address of the contract that originated transfers.
    ///
    /// Format: Prefixed account address - 20 bytes - 0x1234567890123456789012345678901234567890
    pub contract_address: Address,

    /// Identifier of the Ethereum function that originated transfers.
    ///
    /// Format: Prefixed function signature - 4 bytes - 0x12345678
    pub function_id: [u8; 4],

    /// Number of the block that originated transfers.
    ///
    /// Format: Integer (base 10)
    pub block_number: BlockNumber,

    /// List of transfers the `primary_account_address` is part of.
    pub transfers: Vec<AccountTransfer>,
}

/// Represents a token transfer between a debit party and a credit party that happened inside a transaction.
#[serde_as]
#[derive(DebugAsJson, Serialize)]
pub struct AccountTransfer {
    /// Address of the token contract that executed the transfer between `debit_party_address` and `credit_party_address`.
    ///
    /// It may differ from the `contract_address` because any contract can execute transfers in token contracts.
    ///
    /// Format: Prefixed account address - 20 bytes - 0x1234567890123456789012345678901234567890
    pub token_address: Address,

    /// Address of the account from which the token was subtracted.
    ///
    /// Format: Prefixed account address - 20 bytes - 0x1234567890123456789012345678901234567890
    pub debit_party_address: Address,

    /// Address of the account to which the token was added.
    ///
    /// Format: Prefixed account address - 20 bytes - 0x1234567890123456789012345678901234567890
    pub credit_party_address: Address,

    /// Direction of the transfer relative to the primary account (credit or debit).
    ///
    /// Format: [debit, credit]
    pub direction: AccountTransferDirection,

    /// Amount transferred from debit party to credit party.
    ///
    /// Format: Integer (base 10)
    #[serde_as(as = "DisplayFromStr")]
    pub amount: U256,
}

/// Direction of a transfer relative to the primary account.
#[derive(DebugAsJson, Serialize)]
pub enum AccountTransferDirection {
    /// `primary_account_address` is being credited.
    #[serde(rename = "credit")]
    Credit,

    /// `primary_account_address` is being debited.
    #[serde(rename = "debit")]
    Debit,
}

#[cfg(test)]
mod tests {
    use ethereum_types::U256;
    use serde_json::json;

    use crate::eth::primitives::Address;
    use crate::ext::to_json_value;
    use crate::ledger::events::AccountTransfer;
    use crate::ledger::events::AccountTransferDirection;

    #[test]
    fn serde_event_account_transfer() {
        let transfer = AccountTransfer {
            token_address: Address::ZERO,
            debit_party_address: Address::ZERO,
            credit_party_address: Address::ZERO,
            amount: U256::max_value(),
            direction: AccountTransferDirection::Credit,
        };
        let expected = json!( {
            "token_address": "0x0000000000000000000000000000000000000000",
            "debit_party_address": "0x0000000000000000000000000000000000000000",
            "credit_party_address": "0x0000000000000000000000000000000000000000",
            "direction": "credit",
            "amount": "115792089237316195423570985008687907853269984665640564039457584007913129639935"
        });
        assert_eq!(to_json_value(&transfer), expected);
    }

    #[test]
    fn serde_event_account_transfer_direction() {
        assert_eq!(to_json_value(&AccountTransferDirection::Credit), json!("credit"));
        assert_eq!(to_json_value(&AccountTransferDirection::Debit), json!("debit"));
    }
}
