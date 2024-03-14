use std::fs::File;

use anyhow::Context;

use crate::eth::primitives::Account;
use crate::eth::primitives::Block;
use crate::eth::storage::StorageError;

/// Export primitives to CSV files.
pub struct CsvExporter {
    accounts: csv::Writer<File>,
    transactions: csv::Writer<File>,
}

impl Default for CsvExporter {
    fn default() -> Self {
        Self {
            accounts: csv_writer("data/accounts.csv"),
            transactions: csv_writer("data/transactions.csv"),
        }
    }
}

fn csv_writer(path: &'static str) -> csv::Writer<File> {
    csv::WriterBuilder::new()
        .has_headers(true)
        .delimiter(b'\t')
        .quote_style(csv::QuoteStyle::NonNumeric)
        .from_path(path)
        .unwrap()
}

impl CsvExporter {
    pub async fn export_block(&mut self, block: Block) -> anyhow::Result<(), StorageError> {
        for tx in block.transactions {
            let row = [
                tx.input.hash.to_string(),
                tx.input.nonce.to_string(),
                tx.input.from.to_string(),
                tx.input.to.map(|x| x.to_string()).unwrap_or_default(),
                tx.input.value.to_string(),
            ];
            self.transactions.write_record(row).context("failed to write csv transaction")?;
        }

        Ok(())
    }

    pub async fn export_accounts(&mut self, accounts: Vec<Account>) -> anyhow::Result<()> {
        for account in accounts {
            self.accounts.serialize(account).context("failed to write account")?;
        }
        Ok(())
    }
}
