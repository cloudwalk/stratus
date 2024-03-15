use std::fs::File;

use anyhow::Context;

use crate::eth::primitives::Account;
use crate::eth::primitives::Block;
use crate::eth::primitives::TransactionMined;

const ACCOUNTS_HEADERS: [&str; 7] = [
    "address",
    "bytecode",
    "latest_balance",
    "latest_nonce",
    "creation_block",
    "previous_balance",
    "previous_nonce",
];

const TRANSACTION_HEADERS: [&str; 17] = [
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
];

/// Export primitives to CSV files.
pub struct CsvExporter {
    accounts: csv::Writer<File>,
    transactions: csv::Writer<File>,
}

impl CsvExporter {
    /// Creates a new [`CsvExporter`] with all defaults CSVs in the default export path.
    pub fn new() -> anyhow::Result<Self> {
        let exporter = Self {
            accounts: csv_writer("data/accounts.csv", &ACCOUNTS_HEADERS)?,

            transactions: csv_writer("data/transactions.csv", &TRANSACTION_HEADERS)?,
        };
        Ok(exporter)
    }
}

fn csv_writer(path: &'static str, headers: &[&'static str]) -> anyhow::Result<csv::Writer<File>> {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(true)
        .delimiter(b'\t')
        .quote_style(csv::QuoteStyle::NonNumeric)
        .from_path(path)
        .context("failed to create csv writer")?;

    writer.write_record(headers).context("fai;ed to write csv header")?;

    Ok(writer)
}

impl CsvExporter {
    pub fn export_block(&mut self, block: Block) -> anyhow::Result<()> {
        export_transactions(&mut self.transactions, block.transactions)?;
        Ok(())
    }

    pub fn export_initial_accounts(&mut self, accounts: Vec<Account>) -> anyhow::Result<()> {
        for account in accounts {
            let row = [
                account.address.to_string(),                                 // address
                account.bytecode.map(|x| x.to_string()).unwrap_or_default(), // bytecode
                account.balance.to_string(),                                 // latest_balance
                account.nonce.to_string(),                                   // latest_nonce
                "0".to_owned(),                                              // creation_block
                "0".to_owned(),                                              // previous_balance
                "0".to_owned(),                                              // previous_nonce
            ];
            self.accounts.write_record(row).context("failed to write csv transaction")?;
        }
        Ok(())
    }
}

fn export_transactions(writer: &mut csv::Writer<File>, transactions: Vec<TransactionMined>) -> anyhow::Result<()> {
    for tx in transactions {
        let row = [
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
        ];

        writer.write_record(row).context("failed to write csv transaction")?;
    }
    Ok(())
}
