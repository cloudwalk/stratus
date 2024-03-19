use std::fs;
use std::fs::File;

use anyhow::Context;
use anyhow::Ok;
use byte_unit::Byte;
use byte_unit::Unit;
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

const ACCOUNTS_FILE: &str = "data/accounts";

const ACCOUNTS_HEADERS: [&str; 10] = [
    "id",
    "address",
    "bytecode",
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

const TRANSACTIONS_FILE: &str = "data/transactions";

const TRANSACTIONS_HEADERS: [&str; 20] = [
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

const LOGS_HEADERS: [&str; 10] = [
    "id",
    "address",
    "data",
    "transaction_hash",
    "transaction_idx",
    "log_idx",
    "block_number",
    "block_hash",
    "created_at",
    "updated_at",
];

// -----------------------------------------------------------------------------
// Exporter
// -----------------------------------------------------------------------------

/// Export CSV files in the same format of the PostgreSQL tables.
pub struct CsvExporter {
    staged_accounts: Vec<Account>,
    staged_blocks: Vec<Block>,

    accounts_csv: csv::Writer<File>,
    accounts_id: LastId,

    historical_slots_csv: csv::Writer<File>,
    historical_slots_id: LastId,

    historical_balances_csv: csv::Writer<File>,
    historical_balances_id: LastId,

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
        Ok(Self {
            staged_blocks: Vec::new(),
            staged_accounts: Vec::new(),

            accounts_csv: csv_writer(ACCOUNTS_FILE, BlockNumber::ZERO, &ACCOUNTS_HEADERS)?,
            accounts_id: LastId::new_zero(ACCOUNTS_FILE),

            historical_slots_csv: csv_writer(HISTORICAL_SLOTS_FILE, number, &HISTORICAL_SLOTS_HEADERS)?,
            historical_slots_id: LastId::new(HISTORICAL_SLOTS_FILE)?,

            historical_balances_csv: csv_writer(HISTORICAL_BALANCES_FILE, number, &HISTORICAL_BALANCES_HEADERS)?,
            historical_balances_id: LastId::new(HISTORICAL_BALANCES_FILE)?,

            transactions_csv: csv_writer(TRANSACTIONS_FILE, number, &TRANSACTIONS_HEADERS)?,
            transactions_id: LastId::new(TRANSACTIONS_FILE)?,

            blocks_csv: csv_writer(BLOCKS_FILE, number, &BLOCKS_HEADERS)?,
            blocks_id: LastId::new(BLOCKS_FILE)?,

            logs_csv: csv_writer(LOGS_FILE, number, &LOGS_HEADERS)?,
            logs_id: LastId::new(LOGS_FILE)?,
        })
    }

    // -------------------------------------------------------------------------
    // Stagers
    // -------------------------------------------------------------------------

    /// Add an account to be exported.
    pub fn add_account(&mut self, account: Account) -> anyhow::Result<()> {
        self.staged_accounts.push(account);
        Ok(())
    }

    /// Add a block to be exported.
    pub fn add_block(&mut self, block: Block) -> anyhow::Result<()> {
        self.staged_blocks.push(block);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Exporters
    // -------------------------------------------------------------------------
    pub fn flush(&mut self) -> anyhow::Result<()> {
        // export accounts
        let accounts = self.staged_accounts.drain(..).collect_vec();
        self.export_accounts(accounts)?;

        // export blocks
        let blocks = self.staged_blocks.drain(..).collect_vec();
        for block in blocks {
            self.export_account_changes(block.compact_account_changes(), block.number())?;
            self.export_transactions(block.transactions)?;
            self.export_blocks(block.header)?;
        }

        // flush pending data
        self.historical_slots_csv.flush()?;
        self.historical_slots_id.save()?;

        self.historical_balances_csv.flush()?;
        self.historical_balances_id.save()?;

        self.transactions_csv.flush()?;
        self.transactions_id.save()?;

        self.blocks_csv.flush()?;
        self.blocks_id.save()?;

        self.logs_csv.flush()?;
        self.logs_id.save()?;

        Ok(())
    }

    fn export_accounts(&mut self, accounts: Vec<Account>) -> anyhow::Result<()> {
        for account in accounts {
            self.accounts_id.value += 1;
            let row = [
                self.accounts_id.value.to_string(),                          // id
                account.address.to_string(),                                 // address
                account.bytecode.map(|x| x.to_string()).unwrap_or_default(), // bytecode
                account.balance.to_string(),                                 // latest_balance
                account.nonce.to_string(),                                   // latest_nonce
                "0".to_owned(),                                              // creation_block
                "0".to_owned(),                                              // previous_balance
                "0".to_owned(),                                              // previous_nonce
                now(),                                                       // created_at
                now(),                                                       // updated_at
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
            let record = [
                self.transactions_id.value.to_string(),                 // id
                tx.input.hash.to_string(),                              // hash
                tx.input.from.to_string(),                              // signer_address
                tx.input.nonce.to_string(),                             // nonce
                tx.input.from.to_string(),                              // address_from
                tx.input.to.map(|x| x.to_string()).unwrap_or_default(), // address_to
                tx.input.input.to_string(),                             // input
                tx.execution.output.to_string(),                        // output
                tx.input.gas_limit.to_string(),                         // gas
                tx.input.gas_price.to_string(),                         // gas_price
                tx.transaction_index.to_string(),                       // idx_in_block
                tx.block_number.to_string(),                            // block_number
                tx.block_hash.to_string(),                              // block_hash
                tx.input.v.to_string(),                                 // v
                tx.input.r.to_string(),                                 // r
                tx.input.s.to_string(),                                 // s
                tx.input.value.to_string(),                             // value
                tx.execution.result.to_string(),                        // result
                now.clone(),                                            // created_at
                now,                                                    // updated_at
            ];
            self.transactions_csv.write_record(record).context("failed to write csv transaction")?;
        }
        Ok(())
    }

    fn export_blocks(&mut self, block: BlockHeader) -> anyhow::Result<()> {
        self.blocks_id.value += 1;

        let now = now();
        let record = [
            self.blocks_id.value.to_string(),    // id
            block.number.to_string(),            // number
            block.hash.to_string(),              // hash
            block.transactions_root.to_string(), // transactions_root
            block.gas_limit.to_string(),         // gas_limit
            block.gas_used.to_string(),          // gas_used
            block.bloom.to_string(),             // logs_bloom
            block.timestamp.to_string(),         // timestamp_in_secs
            block.parent_hash.to_string(),       // parent_hash
            block.author.to_string(),            // author
            block.extra_data.to_string(),        // extra_data
            block.miner.to_string(),             // miner
            block.difficulty.to_string(),        // difficulty
            block.receipts_root.to_string(),     // receipts_root
            block.uncle_hash.to_string(),        // uncle_hash
            block.size.to_string(),              // size
            block.state_root.to_string(),        // state_root
            block.total_difficulty.to_string(),  // total_difficulty
            block.nonce.to_string(),             // nonce
            now.clone(),                         // created_at
            now,                                 // updated_at
        ];

        self.blocks_csv.write_record(record).context("failed to write csv block")?;

        Ok(())
    }

    fn export_account_changes(&mut self, changes: Vec<ExecutionAccountChanges>, block_number: &BlockNumber) -> anyhow::Result<()> {
        for change in changes {
            // historical_nonces
            if let Some(_nonce) = change.nonce.take_modified() {
                // todo: export historical nonce
            }

            // historical_balances
            if let Some(balance) = change.balance.take_modified() {
                let now = now();
                self.historical_balances_id.value += 1;
                let row = [
                    self.historical_balances_id.value.to_string(), // id
                    change.address.to_string(),                    // address
                    balance.to_string(),                           // balance
                    block_number.to_string(),                      // block_number
                    now.clone(),                                   // updated_at
                    now,                                           // created_at
                ];
                self.historical_balances_csv
                    .write_record(row)
                    .context("failed to write csv historical balances")?;
            }

            // historical_slots
            for slot in change.slots.into_values() {
                if let Some(slot) = slot.take_modified() {
                    let now = now();
                    self.historical_slots_id.value += 1;
                    let row = [
                        self.historical_slots_id.value.to_string(), // id
                        slot.index.to_string(),                     // idx
                        slot.value.to_string(),                     // value
                        block_number.to_string(),                   // block_number
                        change.address.to_string(),                 // account_address
                        now.clone(),                                // updated_at
                        now,                                        // created_at
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

            let now = now();
            let record = [
                self.logs_id.value.to_string(),    // id
                log.address().to_string(),         // address
                log.log.data.to_string(),          // data
                log.transaction_hash.to_string(),  // transaction_hash
                log.transaction_index.to_string(), // transaction_idx
                log.log_index.to_string(),         // log_idx
                log.block_number.to_string(),      // block_number
                log.block_hash.to_string(),        // block_hash
                now.clone(),                       // created_at
                now,                               // updated_at
            ];
            self.logs_csv.write_record(record).context("failed to write csv transaction log")?;
        }
        Ok(())
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
        if fs::metadata(&id.file).is_ok() {
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

/// Creates a new CSV writer at the specified path. If the file exists, it will overwrite it.
fn csv_writer(base_path: &'static str, number: BlockNumber, headers: &[&'static str]) -> anyhow::Result<csv::Writer<File>> {
    let path = format!("{}-{}.csv", base_path, number);
    let buffer_size = Byte::from_u64_with_unit(4, Unit::MiB).unwrap();
    let mut writer = csv::WriterBuilder::new()
        .has_headers(true)
        .delimiter(b'\t')
        .quote_style(csv::QuoteStyle::Always)
        .buffer_capacity(buffer_size.as_u64() as usize)
        .from_path(path)
        .context("failed to create csv writer")?;

    writer.write_record(headers).context("fai;ed to write csv header")?;

    Ok(writer)
}

/// Returns the current date formatted for the CSV file.
fn now() -> String {
    let now = chrono::Utc::now();
    now.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}
