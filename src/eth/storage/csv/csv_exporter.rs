use std::fs;
use std::fs::File;

use anyhow::Context;
use anyhow::Ok;
use itertools::Itertools;

use crate::eth::primitives::Account;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::TransactionMined;

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

const ACCOUNT_FILE: &str = "data/accounts";

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

// -----------------------------------------------------------------------------
// Exporter
// -----------------------------------------------------------------------------

/// Export CSV files in the same format of the PostgreSQL tables.
pub struct CsvExporter {
    staged_accounts: Vec<Account>,
    staged_blocks: Vec<Block>,

    accounts_csv: csv::Writer<File>,
    accounts_id: usize,

    transactions_csv: csv::Writer<File>,
    transactions_id: usize,
}

impl CsvExporter {
    /// Creates a new [`CsvExporter`].
    pub fn new(number: BlockNumber) -> anyhow::Result<Self> {
        Ok(Self {
            staged_blocks: Vec::new(),
            staged_accounts: Vec::new(),

            accounts_csv: csv_writer(ACCOUNT_FILE, BlockNumber::ZERO, &ACCOUNTS_HEADERS)?,
            accounts_id: 0,

            transactions_csv: csv_writer(TRANSACTIONS_FILE, number, &TRANSACTIONS_HEADERS)?,
            transactions_id: read_csv_last_id(TRANSACTIONS_FILE)?,
        })
    }
}

impl CsvExporter {
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
        Ok(())
    }

    fn export_accounts(&mut self, accounts: Vec<Account>) -> anyhow::Result<()> {
        for account in accounts {
            self.accounts_id += 1;
            let row = [
                self.accounts_id.to_string(),                                // id
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
            self.transactions_id += 1;
            let row = [
                self.transactions_id.to_string(),                       // id
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
                now(),                                                  // created_at
                now(),                                                  // updated_at
            ];
            self.transactions_csv.write_record(row).context("failed to write csv transaction")?;
        }
        self.transactions_csv.flush()?;
        write_csv_last_id(TRANSACTIONS_FILE, self.transactions_id)?;
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Creates a new CSV writer at the specified path. If the file exists, it will overwrite it.
fn csv_writer(base_path: &'static str, number: BlockNumber, headers: &[&'static str]) -> anyhow::Result<csv::Writer<File>> {
    let path = format!("{}-{}.csv", base_path, number);
    let mut writer = csv::WriterBuilder::new()
        .has_headers(true)
        .delimiter(b'\t')
        .quote_style(csv::QuoteStyle::Always)
        .from_path(path)
        .context("failed to create csv writer")?;

    writer.write_record(headers).context("fai;ed to write csv header")?;

    Ok(writer)
}

/// Reads the last id saved to a CSV file.
fn read_csv_last_id(base_path: &'static str) -> anyhow::Result<usize> {
    let file = format!("{}-last-id.txt", base_path);

    // when file does not exist, assume 0
    if fs::metadata(file.clone()).is_err() {
        return Ok(0);
    }

    // when file exists, read the last id from file
    let content = fs::read_to_string(file).context("failed to read last_id file")?;
    let id = content.parse().context("failed to parse last_id file content")?;
    Ok(id)
}

fn write_csv_last_id(base_path: &'static str, id: usize) -> anyhow::Result<()> {
    let file = format!("{}-last-id.txt", base_path);
    fs::write(file, id.to_string()).context("failed to write last_id file")?;
    Ok(())
}

/// Returns the current date formatted for the CSV file.
fn now() -> String {
    let now = chrono::Utc::now();
    now.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}
