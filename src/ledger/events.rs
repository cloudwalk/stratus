use std::collections::HashSet;

use chrono::DateTime;
use chrono::Utc;
use display_json::DebugAsJson;
use ethereum_types::H256;
use ethereum_types::U256;
use hex_literal::hex;
use itertools::Itertools;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::ser::SerializeStruct;
use serde::Serialize;
use uuid::Uuid;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogTopic;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::UnixTime;
use crate::ext::InfallibleExt;
use crate::if_else;

/// ERC-20 tokens are configured with 6 decimal places, so it is necessary to divide them by 1.000.000 to get the human readable currency amount.
const TOKEN_SCALE: Decimal = Decimal::from_parts(1_000_000, 0, 0, false, 0);

/// Represents token transfers (debits and credits) associated with a specific Ethereum account within a single transaction.
///
/// The `account_address` field identifies the primary account involved in these transfers.
///
/// An event will be generated for each account involved in a transaction, meaning if a transaction is not the primary in this event,
/// in another event it will treated as the primary and credit and debit operations adjusted accordingly.
///
/// A single event can contain multiple token transfers (e.g., a customer is debited for a card payment but receives a credit as cashback)
#[derive(DebugAsJson)]
pub struct AccountTransfers {
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

    /// Address of the account that is part of all transfers. Also referenced as primary account address.
    ///
    /// Format: Prefixed account address - 20 bytes - 0x1234567890123456789012345678901234567890
    pub account_address: Address,

    /// Hash of the Ethereum transaction that originated transfers.
    ///
    /// Format: Prefixed transaction hash - 32 bytes - 0x1234567890123456789012345678901234567890123456789012345678901234
    pub transaction_hash: Hash,

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
    /// Format: Number in base 10 - Range: 0 to [`u64::MAX`]
    pub block_number: BlockNumber,

    /// Datetime of the Ethereum block that originated transfers.
    ///
    /// Format: ISO 8601
    pub block_datetime: DateTime<Utc>,

    /// List of transfers the `account_address` is part of.
    pub transfers: Vec<AccountTransfer>,
}

impl AccountTransfers {
    /// Idempotency key of the event payload.
    ///
    /// It is unique for each distinct event, but consistent across retries or republications of the same event payload.
    ///
    /// Format: String - transaction_hash::account_address - 0x1234567890123456789012345678901234567890123456789012345678901234::0x1234567890123456789012345678901234567890
    pub fn idempotency_key(&self) -> String {
        format!("{}::{}", self.transaction_hash, self.account_address)
    }
}

/// Represents a token transfer between a debit party and a credit party that happened inside a transaction.
#[derive(DebugAsJson)]
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

    /// Direction of the transfer relative to the primary account address (credit or debit).
    ///
    /// Format: [debit, credit]
    pub direction: AccountTransferDirection,

    /// Amount transferred from debit party to credit party.
    ///
    /// Format: Number in base 10 and 6 decimal places - Formatted as String to avoid losing precision - Range: 0 to 18446744073709.551615.
    pub amount: U256,
}

/// Direction of a transfer relative to the primary account address.
#[derive(DebugAsJson, strum::EnumIs)]
pub enum AccountTransferDirection {
    /// `account_address` is being credited.
    Credit,

    /// `account_address` is being debited.
    Debit,
}

// -----------------------------------------------------------------------------
// Serializers
// -----------------------------------------------------------------------------

impl Serialize for AccountTransfers {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("AccountTransfersEvent", 10)?;

        state.serialize_field("publication_id", &self.publication_id.to_string())?;
        state.serialize_field("publication_datetime", &self.publication_datetime.to_rfc3339())?;
        state.serialize_field("idempotency_key", &self.idempotency_key())?;
        state.serialize_field("account_address", &self.account_address)?;
        state.serialize_field("transaction_hash", &self.transaction_hash)?;
        state.serialize_field("contract_address", &self.contract_address)?;
        state.serialize_field("function_id", &const_hex::encode_prefixed(self.function_id))?;
        state.serialize_field("block_number", &self.block_number.as_u64())?;
        state.serialize_field("block_datetime", &self.block_datetime.to_rfc3339())?;
        state.serialize_field("transfers", &self.transfers)?;
        state.end()
    }
}

impl Serialize for AccountTransfer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let amount = Decimal::from_u64(self.amount.low_u64()).expect_infallible() / TOKEN_SCALE;

        let mut state = serializer.serialize_struct("AccountTransfer", 5)?;
        state.serialize_field("token_address", &self.token_address)?;
        state.serialize_field("debit_party_address", &self.debit_party_address)?;
        state.serialize_field("credit_party_address", &self.credit_party_address)?;
        state.serialize_field("amount", &amount)?;
        state.serialize_field("direction", &self.direction)?;
        state.end()
    }
}

impl Serialize for AccountTransferDirection {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Credit => serializer.serialize_str("credit"),
            Self::Debit => serializer.serialize_str("debit"),
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions
// -----------------------------------------------------------------------------

/// ERC-20 transfer event hash.
const TRANSFER_EVENT: LogTopic = LogTopic(H256(hex!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef")));

/// Converts a mined transaction into multiple account transfers events to be published.
pub fn transaction_to_events(block_timestamp: UnixTime, tx: TransactionMined) -> Vec<AccountTransfers> {
    // identify token transfers in transaction
    let transfers = tx
        .logs
        .into_iter()
        .filter(|log| log.log.topic0.is_some_and(|topic0| topic0 == TRANSFER_EVENT))
        .filter_map(|log| {
            let amount_bytes: [u8; 32] = match log.log.data.0.try_into() {
                Ok(bytes) => bytes,
                Err(_) => {
                    tracing::error!("bug: event identified as ERC-20 transfer should have the amount as 32 bytes in the data field");
                    return None;
                }
            };

            let token = log.log.address;
            let from: Address = log.log.topic1?.into();
            let to: Address = log.log.topic2?.into();
            let amount = U256::from_big_endian(&amount_bytes); // TODO: review

            Some((token, from, to, amount))
        })
        .collect_vec();

    // identify accounts involved in transfers
    let mut accounts = HashSet::new();
    for (_, from, to, _) in &transfers {
        accounts.insert(from);
        accounts.insert(to);
    }

    // for each account, generate an event
    let mut events = Vec::with_capacity(accounts.len());
    for account in accounts {
        // generate base event
        let mut event = AccountTransfers {
            publication_id: Uuid::now_v7(),
            publication_datetime: Utc::now(),
            account_address: *account,
            transaction_hash: tx.input.hash,
            contract_address: tx.input.to.unwrap_or_else(|| {
                tracing::error!("bug: transaction emitting transfers must have the contract address");
                Address::ZERO
            }),
            function_id: tx.input.input[0..4].try_into().unwrap_or_else(|_| {
                tracing::error!("bug: transaction emitting transfers must have the 4-byte signature");
                [0; 4]
            }),
            block_number: tx.block_number,
            block_datetime: block_timestamp.into(),
            transfers: vec![],
        };

        // generate transfers
        for (token, from, to, amount) in &transfers {
            if account != from && account != to {
                continue;
            }
            let direction = if_else!(account == from, AccountTransferDirection::Debit, AccountTransferDirection::Credit);
            let transfer = AccountTransfer {
                token_address: *token,
                debit_party_address: *from,
                credit_party_address: *to,
                direction,
                amount: *amount,
            };
            event.transfers.push(transfer);
        }
        events.push(event);
    }

    events
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use chrono::Utc;
    use ethereum_types::U256;
    use fake::Fake;
    use fake::Faker;
    use serde_json::json;
    use uuid::Uuid;

    use crate::eth::primitives::test_accounts;
    use crate::eth::primitives::Address;
    use crate::eth::primitives::BlockNumber;
    use crate::eth::primitives::Bytes;
    use crate::eth::primitives::Hash;
    use crate::eth::primitives::LogMined;
    use crate::eth::primitives::TransactionMined;
    use crate::eth::primitives::UnixTime;
    use crate::ext::to_json_value;
    use crate::ledger::events::transaction_to_events;
    use crate::ledger::events::AccountTransfer;
    use crate::ledger::events::AccountTransferDirection;
    use crate::ledger::events::AccountTransfers;
    use crate::ledger::events::TRANSFER_EVENT;

    #[test]
    fn ledger_events_serde_account_transfers() {
        let event = AccountTransfers {
            publication_id: Uuid::nil(),
            publication_datetime: "2024-10-16T19:47:50Z".parse().unwrap(),
            account_address: Address::ZERO,
            transaction_hash: Hash::ZERO,
            block_datetime: "2024-10-16T19:47:50Z".parse().unwrap(),
            contract_address: Address::ZERO,
            function_id: [0, 0, 0, 0],
            block_number: BlockNumber::ZERO,
            transfers: vec![AccountTransfer {
                token_address: Address::ZERO,
                debit_party_address: Address::ZERO,
                credit_party_address: Address::ZERO,
                amount: U256::max_value(),
                direction: AccountTransferDirection::Credit,
            }],
        };
        let expected = json!(
            {
                "publication_id": "00000000-0000-0000-0000-000000000000",
                "publication_datetime": "2024-10-16T19:47:50+00:00",
                "idempotency_key": "0x0000000000000000000000000000000000000000000000000000000000000000::0x0000000000000000000000000000000000000000",
                "account_address": "0x0000000000000000000000000000000000000000",
                "transaction_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "contract_address":"0x0000000000000000000000000000000000000000",
                "function_id": "0x00000000",
                "block_number": 0,
                "block_datetime": "2024-10-16T19:47:50+00:00",
                "transfers": [{
                    "token_address": "0x0000000000000000000000000000000000000000",
                    "debit_party_address": "0x0000000000000000000000000000000000000000",
                    "credit_party_address": "0x0000000000000000000000000000000000000000",
                    "direction": "credit",
                    "amount": "18446744073709.551615"
                }],
            }
        );
        assert_eq!(to_json_value(&event), expected);
    }

    #[test]
    fn ledger_events_serde_event_account_transfer_direction() {
        assert_eq!(to_json_value(&AccountTransferDirection::Credit), json!("credit"));
        assert_eq!(to_json_value(&AccountTransferDirection::Debit), json!("debit"));
    }

    #[test]
    fn ledger_events_parse_transfer_events() {
        // reference values
        let accounts = test_accounts();
        let alice = &accounts[0];
        let bob = &accounts[1];
        let charlie = &accounts[2];
        let token_address = Address::BRLC;
        let amount_bytes = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255,
        ];
        let amount_u256 = U256::from_big_endian(&amount_bytes);

        // 1. generate fake block data transaction and block data
        let block_timestamp: UnixTime = 1729108070.into();

        // 2. generate fake tx data
        let mut tx: TransactionMined = Fake::fake(&Faker);
        tx.input.input = Bytes(vec![1, 2, 3, 4, 5, 6, 7, 8]);

        let mut log_transfer1: LogMined = Fake::fake(&Faker);
        log_transfer1.log.address = token_address;
        log_transfer1.log.topic0 = Some(TRANSFER_EVENT);
        log_transfer1.log.topic1 = Some(alice.address.into());
        log_transfer1.log.topic2 = Some(bob.address.into());
        log_transfer1.log.data = Bytes(amount_bytes.to_vec());

        let mut log_transfer2: LogMined = Fake::fake(&Faker);
        log_transfer2.log.address = token_address;
        log_transfer2.log.topic0 = Some(TRANSFER_EVENT);
        log_transfer2.log.topic1 = Some(bob.address.into());
        log_transfer2.log.topic2 = Some(charlie.address.into());
        log_transfer2.log.data = Bytes(amount_bytes.to_vec());

        let log_random: LogMined = Fake::fake(&Faker);

        tx.logs.push(log_transfer1);
        tx.logs.push(log_random);
        tx.logs.push(log_transfer2);

        // 3. parse events
        let events = transaction_to_events(block_timestamp, tx.clone());

        // 4. assert events
        assert_eq!(events.len(), 3); // number of accounts involved in all transactions
        for event in events {
            assert_eq!(&event.transaction_hash, &tx.input.hash);
            assert_eq!(&event.contract_address, &tx.input.to.unwrap());
            assert_eq!(&event.function_id[0..], &tx.input.input.0[0..4]);
            assert_eq!(&event.block_number, &tx.block_number);
            assert_eq!(&event.block_datetime, &DateTime::<Utc>::from(block_timestamp));

            // assert transfers
            match event.account_address {
                a if a == alice.address => assert_eq!(event.transfers.len(), 1),
                a if a == bob.address => assert_eq!(event.transfers.len(), 2),
                a if a == charlie.address => assert_eq!(event.transfers.len(), 1),
                _ => panic!("invalid account"),
            }
            for transfer in event.transfers {
                assert_eq!(transfer.token_address, token_address);

                assert!(event.account_address == transfer.credit_party_address || event.account_address == transfer.debit_party_address);
                if transfer.direction.is_credit() {
                    assert_eq!(event.account_address, transfer.credit_party_address);
                } else {
                    assert_eq!(event.account_address, transfer.debit_party_address);
                }
                assert_eq!(transfer.amount, amount_u256);

                // assert json format
                let transfer_json = serde_json::to_value(transfer).unwrap();
                assert_eq!(*transfer_json.get("amount").unwrap(), json!("0.065535"));
            }
        }
    }
}
