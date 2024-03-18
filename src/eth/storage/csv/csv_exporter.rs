use std::fs;
use std::fs::File;

use anyhow::Context;
use anyhow::Ok;
use byte_unit::Byte;
use byte_unit::Unit;
use itertools::Itertools;

use crate::eth::primitives::Account;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::ExecutionAccountChanges;

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

const HISTORICAL_BALANCES_HEADERS: [&str; 6] = [
    "id",
    "address",
    "balance",
    "block_number",
    "created_at",
    "updated_at",
];

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

    historical_balances_csv: csv::Writer<File>,
    historical_balances_id: LastId,
    
    transactions_csv: csv::Writer<File>,
    transactions_id: LastId,

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

            historical_balances_csv: csv_writer(HISTORICAL_BALANCES_FILE, number, &HISTORICAL_BALANCES_HEADERS)?,
            historical_balances_id: LastId::new(HISTORICAL_BALANCES_FILE)?,

            transactions_csv: csv_writer(TRANSACTIONS_FILE, number, &TRANSACTIONS_HEADERS)?,
            transactions_id: LastId::new(TRANSACTIONS_FILE)?,

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
            self.export_transactions(block.transactions)?;
        }

        // i know this is not the best way to do this, but i'm not sure how to do it better
        let blocks2 = self.staged_blocks.drain(..).collect_vec();
        for block in blocks2 {
            self.export_historical_balances(block.compact_account_changes(), block.number())?;
        }

        self.historical_balances_csv.flush()?;
        self.historical_balances_id.save()?;


        // flush pending data
        self.transactions_csv.flush()?;
        self.transactions_id.save()?;

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

    fn export_historical_balances(&mut self, account_changes: Vec<ExecutionAccountChanges>, block_number: &BlockNumber) -> anyhow::Result<()> {
        self.historical_balances_id.value += 1;
        for account_change in account_changes {
            let row = [
                self.historical_balances_id.value.to_string(),          // id
                account_change.address.to_string(),                     // address
                account_change.balance.to_string(),                     // balance
                block_number.to_string(),                               // block_number
                now(),                                                  // created_at
                now(),                                                  // updated_at
            ];
            self.historical_balances_csv.write_record(row).context("failed to write csv historical balances")?;
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
