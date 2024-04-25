use std::fmt::Display;
use std::fs;
use std::fs::File;
use std::path::Path;

use anyhow::Context;
use anyhow::Ok;
use byte_unit::Byte;
use byte_unit::Unit;
use const_hex::hex;
use ethereum_types::U256;
use itertools::Itertools;

use crate::eth::primitives::Account;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionMined;

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

const NULL: &str = ""; // TODO: how?

const ACCOUNTS_FILE: &str = "data/accounts";

const ACCOUNTS_HEADERS: [&str; 11] = [
    "id",
    "address",
    "bytecode",
    "code_hash",
    "latest_balance",
    "latest_nonce",
    "creation_block",
    "previous_balance",
    "previous_nonce",
    "created_at",
    "updated_at",
];

const HISTORICAL_BALANCES_FILE: &str = "data/historical_balances";

const HISTORICAL_SLOTS_FILE: &str = "data/historical_slots";

const HISTORICAL_SLOTS_HEADERS: [&str; 7] = ["id", "idx", "value", "block_number", "account_address", "created_at", "updated_at"];

const HISTORICAL_BALANCES_HEADERS: [&str; 6] = ["id", "address", "balance", "block_number", "created_at", "updated_at"];

const HISTORICAL_NONCES_FILE: &str = "data/historical_nonces";

const HISTORICAL_NONCES_HEADERS: [&str; 6] = ["id", "address", "nonce", "block_number", "created_at", "updated_at"];

const TRANSACTIONS_FILE: &str = "data/transactions";

const TRANSACTIONS_HEADERS: [&str; 21] = [
    "id",
    "hash",
    "signer_address",
    "nonce",
    "address_from",
    "address_to",
    "input",
    "output",
    "gas",
    "gas_price",
    "idx_in_block",
    "block_number",
    "block_hash",
    "v",
    "r",
    "s",
    "value",
    "result",
    "created_at",
    "updated_at",
    "chain_id",
];

const BLOCKS_FILE: &str = "data/blocks";

const BLOCKS_HEADERS: [&str; 21] = [
    "id",
    "number",
    "hash",
    "transactions_root",
    "gas_limit",
    "gas_used",
    "logs_bloom",
    "timestamp_in_secs",
    "parent_hash",
    "author",
    "extra_data",
    "miner",
    "difficulty",
    "receipts_root",
    "uncle_hash",
    "size",
    "state_root",
    "total_difficulty",
    "nonce",
    "created_at",
    "updated_at",
];

const LOGS_FILE: &str = "data/logs";

const LOGS_HEADERS: [&str; 14] = [
    "id",
    "address",
    "data",
    "transaction_hash",
    "transaction_idx",
    "log_idx",
    "block_number",
    "block_hash",
    "topic0",
    "topic1",
    "topic2",
    "topic3",
    "created_at",
    "updated_at",
];

// -----------------------------------------------------------------------------
// Exporter
// -----------------------------------------------------------------------------

/// Export CSV files in the same format of the PostgreSQL tables.
pub struct CsvExporter {
    staged_blocks: Vec<Block>,

    accounts_csv: csv::Writer<File>,
    accounts_id: LastId,

    historical_slots_csv: csv::Writer<File>,
    historical_slots_id: LastId,

    historical_balances_csv: csv::Writer<File>,
    historical_balances_id: LastId,

    historical_nonces_csv: csv::Writer<File>,
    historical_nonces_id: LastId,

    transactions_csv: csv::Writer<File>,
    transactions_id: LastId,

    blocks_csv: csv::Writer<File>,
    blocks_id: LastId,

    logs_csv: csv::Writer<File>,
    logs_id: LastId,
}

impl CsvExporter {
    /// Creates a new [`CsvExporter`].
    pub fn new(number: BlockNumber) -> anyhow::Result<Self> {
        let CsvFiles {
            accounts_csv,
            historical_slots_csv,
            historical_balances_csv,
            historical_nonces_csv,
            transactions_csv,
            blocks_csv,
            logs_csv,
        } = CsvFiles::create(number)?;

        Ok(Self {
            staged_blocks: Vec::new(),

            accounts_csv,
            accounts_id: LastId::new(ACCOUNTS_FILE)?,

            historical_slots_csv,
            historical_slots_id: LastId::new(HISTORICAL_SLOTS_FILE)?,

            historical_balances_csv,
            historical_balances_id: LastId::new(HISTORICAL_BALANCES_FILE)?,

            historical_nonces_csv,
            historical_nonces_id: LastId::new(HISTORICAL_NONCES_FILE)?,

            transactions_csv,
            transactions_id: LastId::new(TRANSACTIONS_FILE)?,

            blocks_csv,
            blocks_id: LastId::new(BLOCKS_FILE)?,

            logs_csv,
            logs_id: LastId::new(LOGS_FILE)?,
        })
    }

    pub fn is_accounts_empty(&self) -> bool {
        self.accounts_id.value == 0
    }

    // -------------------------------------------------------------------------
    // Stagers
    // -------------------------------------------------------------------------

    /// Add a block to be exported.
    pub fn add_block(&mut self, block: Block) -> anyhow::Result<()> {
        self.staged_blocks.push(block);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Exporters
    // -------------------------------------------------------------------------
    pub fn flush(&mut self) -> anyhow::Result<()> {
        // export blocks
        let blocks = self.staged_blocks.drain(..).collect_vec();
        for block in blocks {
            self.export_account_changes(*block.number(), block.compact_account_changes(), block.number())?;
            self.export_block(block.header)?;
            self.export_transactions(block.transactions)?;
        }

        // flush pending data
        self.accounts_csv.flush()?;
        self.accounts_id.save()?;

        self.historical_slots_csv.flush()?;
        self.historical_slots_id.save()?;

        self.historical_balances_csv.flush()?;
        self.historical_balances_id.save()?;

        self.historical_nonces_csv.flush()?;
        self.historical_nonces_id.save()?;

        self.transactions_csv.flush()?;
        self.transactions_id.save()?;

        self.blocks_csv.flush()?;
        self.blocks_id.save()?;

        self.logs_csv.flush()?;
        self.logs_id.save()?;

        Ok(())
    }

    /// Close current files and creates new CSV chunks to write on instead.
    pub fn finish_current_chunks(&mut self, block_number: BlockNumber) -> anyhow::Result<()> {
        self.flush()?;

        let CsvFiles {
            accounts_csv,
            historical_slots_csv,
            historical_balances_csv,
            historical_nonces_csv,
            transactions_csv,
            blocks_csv,
            logs_csv,
        } = CsvFiles::create(block_number)?;

        self.accounts_csv = accounts_csv;
        self.historical_slots_csv = historical_slots_csv;
        self.historical_balances_csv = historical_balances_csv;
        self.historical_nonces_csv = historical_nonces_csv;
        self.transactions_csv = transactions_csv;
        self.blocks_csv = blocks_csv;
        self.logs_csv = logs_csv;

        Ok(())
    }

    pub fn export_initial_accounts(&mut self, accounts: Vec<Account>) -> anyhow::Result<()> {
        for account in accounts {
            self.accounts_id.value += 1;
            let now = now();
            let row = [
                self.accounts_id.value.to_string(),                         // id
                to_bytea(account.address),                                  // address
                account.bytecode.map(to_bytea).unwrap_or(NULL.to_string()), // bytecode
                to_bytea(account.code_hash),                                // code_hash
                account.balance.to_string(),                                // latest_balance
                account.nonce.to_string(),                                  // latest_nonce
                "0".to_owned(),                                             // creation_block
                NULL.to_owned(),                                            // previous_balance
                NULL.to_owned(),                                            // previous_nonce
                now.clone(),                                                // created_at
                now,                                                        // updated_at
            ];
            self.accounts_csv.write_record(row).context("failed to write csv transaction")?;
        }

        Ok(())
    }

    fn export_transactions(&mut self, transactions: Vec<TransactionMined>) -> anyhow::Result<()> {
        for tx in transactions {
            self.transactions_id.value += 1;

            // export relationships
            self.export_logs(tx.logs)?;

            // export data
            let now = now();
            let row = [
                self.transactions_id.value.to_string(),        // id
                to_bytea(tx.input.hash),                       // hash
                to_bytea(tx.input.from),                       // signer_address
                tx.input.nonce.to_string(),                    // nonce
                to_bytea(tx.input.from),                       // address_from
                tx.input.to.map(to_bytea).unwrap_or_default(), // address_to
                to_bytea(tx.input.input),                      // input
                to_bytea(tx.execution.output),                 // output
                tx.execution.gas.to_string(),                  // gas
                tx.input.gas_price.to_string(),                // gas_price
                tx.transaction_index.to_string(),              // idx_in_block
                tx.block_number.to_string(),                   // block_number
                to_bytea(tx.block_hash),                       // block_hash
                to_bytea(tx.input.v.as_u64().to_be_bytes()),   // v
                u256_to_bytea(tx.input.r),                     // r
                u256_to_bytea(tx.input.s),                     // s
                tx.input.value.to_string(),                    // value
                tx.execution.result.to_string(),               // result
                now.clone(),                                   // created_at
                now,                                           // updated_at
                from_option(tx.input.chain_id),                // chain_id
            ];
            self.transactions_csv.write_record(row).context("failed to write csv transaction")?;
        }
        Ok(())
    }

    fn export_block(&mut self, block: BlockHeader) -> anyhow::Result<()> {
        self.blocks_id.value += 1;

        let now = now();

        let row = &[
            self.blocks_id.value.to_string(),   // id
            block.number.to_string(),           // number
            to_bytea(block.hash),               // hash
            to_bytea(block.transactions_root),  // transactions_root
            block.gas_limit.to_string(),        // gas_limit
            block.gas_used.to_string(),         // gas_used
            to_bytea(*block.bloom),             // logs_bloom
            block.timestamp.to_string(),        // timestamp_in_secs
            to_bytea(block.parent_hash),        // parent_hash
            to_bytea(block.author),             // author
            block.extra_data.to_string(),       // extra_data
            to_bytea(block.miner),              // miner
            block.difficulty.to_string(),       // difficulty
            to_bytea(block.receipts_root),      // receipts_root
            to_bytea(block.uncle_hash),         // uncle_hash
            block.size.to_string(),             // size
            to_bytea(block.state_root),         // state_root
            block.total_difficulty.to_string(), // total_difficulty
            to_bytea(block.nonce),              // nonce
            now.clone(),                        // created_at
            now,                                // updated_at
        ];

        self.blocks_csv.write_record(row).context("failed to write csv block")?;

        Ok(())
    }

    fn export_account_changes(&mut self, number: BlockNumber, changes: Vec<ExecutionAccountChanges>, block_number: &BlockNumber) -> anyhow::Result<()> {
        for change in changes {
            let now = now();

            if change.is_account_creation() {
                self.accounts_id.value += 1;
                let change_bytecode = change.bytecode.take_ref().and_then(|x| x.clone().map(to_bytea)).unwrap_or(NULL.to_string());
                let row = [
                    self.accounts_id.value.to_string(),                                                           // id
                    to_bytea(change.address),                                                                     // address
                    change_bytecode,                                                                              // bytecode
                    to_bytea(change.code_hash),                                                                   // code_hash
                    change.balance.take_ref().map(|x| x.to_string()).unwrap_or_default(),                         // latest_balance
                    change.nonce.take_ref().map(|x| x.to_string()).unwrap_or_default(),                           // latest_nonce
                    number.to_string().to_owned(),                                                                // creation_block
                    change.balance.take_original_ref().map(|x| x.to_string()).unwrap_or_else(|| NULL.to_owned()), // previous_balance
                    change.nonce.take_original_ref().map(|x| x.to_string()).unwrap_or_else(|| NULL.to_owned()),   // previous_nonce
                    now.clone(),                                                                                  // created_at
                    now.clone(),                                                                                  // updated_at
                ];
                self.accounts_csv.write_record(row).context("failed to write csv transaction")?;
            }

            // historical_nonces
            if let Some(nonce) = change.nonce.take_modified() {
                self.historical_nonces_id.value += 1;
                let row = [
                    self.historical_nonces_id.value.to_string(), // id
                    to_bytea(change.address),                    // address
                    nonce.to_string(),                           // nonce
                    block_number.to_string(),                    // block_number
                    now.clone(),                                 // updated_at
                    now.clone(),                                 // created_at
                ];
                self.historical_nonces_csv.write_record(row).context("failed to write csv historical nonces")?;
            }
            // historical_balances
            if let Some(balance) = change.balance.take_modified() {
                self.historical_balances_id.value += 1;
                let row = [
                    self.historical_balances_id.value.to_string(), // id
                    to_bytea(change.address),                      // address
                    balance.to_string(),                           // balance
                    block_number.to_string(),                      // block_number
                    now.clone(),                                   // updated_at
                    now.clone(),                                   // created_at
                ];
                self.historical_balances_csv
                    .write_record(row)
                    .context("failed to write csv historical balances")?;
            }

            // historical_slots
            for slot in change.slots.into_values() {
                if let Some(slot) = slot.take_modified() {
                    self.historical_slots_id.value += 1;
                    let row = [
                        self.historical_slots_id.value.to_string(), // id
                        u256_to_bytea(slot.index.as_u256()),        // idx
                        u256_to_bytea(slot.value.as_u256()),        // value
                        block_number.to_string(),                   // block_number
                        to_bytea(change.address),                   // account_address
                        now.clone(),                                // updated_at
                        now.clone(),                                // created_at
                    ];
                    self.historical_slots_csv.write_record(row).context("failed to write csv historical slots")?;
                }
            }
        }
        Ok(())
    }

    fn export_logs(&mut self, logs: Vec<LogMined>) -> anyhow::Result<()> {
        for log in logs {
            self.logs_id.value += 1;

            let get_topic_with_index = |index| log.log.topics().get(index).map(to_bytea).unwrap_or_default();

            let now = now();
            let row = [
                self.logs_id.value.to_string(),    // id
                to_bytea(log.address()),           // address
                to_bytea(&log.log.data),           // data
                to_bytea(log.transaction_hash),    // transaction_hash
                log.transaction_index.to_string(), // transaction_idx
                log.log_index.to_string(),         // log_idx
                log.block_number.to_string(),      // block_number
                to_bytea(log.block_hash),          // block_hash
                get_topic_with_index(0),           // topic0
                get_topic_with_index(1),           // topic1
                get_topic_with_index(2),           // topic2
                get_topic_with_index(3),           // topic3
                now.clone(),                       // created_at
                now,                               // updated_at
            ];
            self.logs_csv.write_record(row).context("failed to write csv transaction log")?;
        }
        Ok(())
    }
}

impl Drop for CsvExporter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

struct LastId {
    file: String,
    value: usize,
}

impl LastId {
    /// Creates a new instance with default zero value.
    fn new_zero(base_path: &'static str) -> Self {
        let file = format!("{}-last-id.txt", base_path);
        Self { file, value: 0 }
    }

    /// Creates a new instance from an existing saved file or assumes default value when file does not exist.
    fn new(base_path: &'static str) -> anyhow::Result<Self> {
        let mut id = Self::new_zero(base_path);

        // when file exist, read value from file
        if Path::new(&id.file).exists() {
            let content = fs::read_to_string(&id.file).context("failed to read last_id file")?;
            id.value = content.parse().context("failed to parse last_id file content")?;
        }

        Ok(id)
    }

    /// Saves the current value to file.
    fn save(&self) -> anyhow::Result<()> {
        fs::write(&self.file, self.value.to_string()).context("failed to write last_id file")?;
        Ok(())
    }
}

struct CsvFiles {
    accounts_csv: csv::Writer<File>,
    historical_slots_csv: csv::Writer<File>,
    historical_balances_csv: csv::Writer<File>,
    historical_nonces_csv: csv::Writer<File>,
    transactions_csv: csv::Writer<File>,
    blocks_csv: csv::Writer<File>,
    logs_csv: csv::Writer<File>,
}

impl CsvFiles {
    fn create(block_number: BlockNumber) -> anyhow::Result<Self> {
        Ok(Self {
            accounts_csv: csv_writer(ACCOUNTS_FILE, block_number, &ACCOUNTS_HEADERS)?,
            historical_slots_csv: csv_writer(HISTORICAL_SLOTS_FILE, block_number, &HISTORICAL_SLOTS_HEADERS)?,
            historical_balances_csv: csv_writer(HISTORICAL_BALANCES_FILE, block_number, &HISTORICAL_BALANCES_HEADERS)?,
            historical_nonces_csv: csv_writer(HISTORICAL_NONCES_FILE, block_number, &HISTORICAL_NONCES_HEADERS)?,
            transactions_csv: csv_writer(TRANSACTIONS_FILE, block_number, &TRANSACTIONS_HEADERS)?,
            blocks_csv: csv_writer(BLOCKS_FILE, block_number, &BLOCKS_HEADERS)?,
            logs_csv: csv_writer(LOGS_FILE, block_number, &LOGS_HEADERS)?,
        })
    }
}

/// Creates a new CSV writer at the specified path. If the file exists, it will overwrite it.
fn csv_writer(base_path: &'static str, number: BlockNumber, headers: &[&'static str]) -> anyhow::Result<csv::Writer<File>> {
    let path = format!("{}-{}.csv", base_path, number);
    let buffer_size = Byte::from_u64_with_unit(4, Unit::MiB).unwrap();
    let mut writer = csv::WriterBuilder::new()
        .has_headers(true)
        .delimiter(b'\t')
        .quote_style(csv::QuoteStyle::Necessary)
        .buffer_capacity(buffer_size.as_u64() as usize)
        .from_path(path)
        .context("failed to create csv writer")?;

    writer.write_record(headers).context("failed to write csv header")?;

    Ok(writer)
}

/// Convert a byte sequence to a `bytea` representation that is parseable by Postgres.
fn to_bytea(bytes: impl AsRef<[u8]>) -> String {
    let hex = hex::encode(bytes.as_ref());
    format!("\\x{hex}")
}

fn u256_to_bytea(integer: U256) -> String {
    let bytes: Vec<u8> = integer.as_ref().iter().copied().rev().flat_map(u64::to_be_bytes).collect();
    to_bytea(bytes)
}

/// Returns the current date formatted for the CSV file.
fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}

/// Builds a string from an optional displayable value.
fn from_option(t: Option<impl Display>) -> String {
    t.map(|x| x.to_string()).unwrap_or_default()
}
