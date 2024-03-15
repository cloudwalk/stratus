use std::fs::File;

use anyhow::Context;

use crate::eth::primitives::Account;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionMined;

const ACCOUNT_SLOTS_HEADERS: [&str; 7] = [
    "address",
    "bytecode",
    "latest_balance",
    "latest_nonce",
    "previous_balance",
    "previous_nonce",
    "creation_block",
];

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

const BLOCKS_HEADERS: [&str; 19] = [
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
];

const HISTORICAL_BALANCES_HEADERS: [&str; 3] = [
    "address",
    "balance",
    "block_number",
];

const HISTORICAL_NONCES_HEADERS: [&str; 3] = [
    "address",
    "nonce",
    "block_number",
];

const HISTORICAL_SLOTS_HEADERS: [&str; 4] = [
    "idx",
    "value",
    "account_address",
    "block_number",
];

const LOGS_HEADERS: [&str; 7] = [
    "address",
    "data",
    "transaction_hash",
    "transaction_idx",
    "log_idx",
    "block_number",
    "block_hash",
];

const TOPICS_HEADERS: [&str; 7] = [
    "topic",
    "transaction_hash",
    "transaction_idx",
    "log_idx",
    "topic_idx",
    "block_number",
    "block_hash",
];

const TRANSACTION_HEADERS: [&str; 20] = [
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

/// Export primitives to CSV files.
pub struct CsvExporter {
    account_slots: csv::Writer<File>,
    account_slots_id: usize,

    accounts: csv::Writer<File>,
    accounts_id: usize,

    blocks: csv::Writer<File>,
    blocks_id: usize,

    historical_balances: csv::Writer<File>,
    historical_balances_id: usize,

    historical_nonces: csv::Writer<File>,
    historical_nonces_id: usize,

    historical_slots: csv::Writer<File>,
    historical_slots_id: usize,

    logs: csv::Writer<File>,
    logs_id: usize,

    topics: csv::Writer<File>,
    topics_id: usize,

    transactions: csv::Writer<File>,
    transactions_id: usize,
}

impl CsvExporter {
    /// Creates a new [`CsvExporter`] with all defaults CSVs in the default export path.
    pub fn new() -> anyhow::Result<Self> {
        let exporter = Self {
            account_slots: csv_writer("data/account_slots.csv", &ACCOUNT_SLOTS_HEADERS)?,
            account_slots_id: 0,

            accounts: csv_writer("data/accounts.csv", &ACCOUNTS_HEADERS)?,
            accounts_id: 0,

            blocks: csv_writer("data/blocks.csv", &BLOCKS_HEADERS)?,
            blocks_id: 0,

            historical_balances: csv_writer("data/historical_balances.csv", &HISTORICAL_BALANCES_HEADERS)?,
            historical_balances_id: 0,

            historical_nonces: csv_writer("data/historical_nonces.csv", &HISTORICAL_NONCES_HEADERS)?,
            historical_nonces_id: 0,

            historical_slots: csv_writer("data/historical_slots.csv", &HISTORICAL_SLOTS_HEADERS)?,
            historical_slots_id: 0,

            logs: csv_writer("data/logs.csv", &LOGS_HEADERS)?,
            logs_id: 0,

            topics: csv_writer("data/topics.csv", &TOPICS_HEADERS)?,
            topics_id: 0,

            transactions: csv_writer("data/transactions.csv", &TRANSACTION_HEADERS)?,
            transactions_id: 0,
        };
        Ok(exporter)
    }
}

fn csv_writer(path: &'static str, headers: &[&'static str]) -> anyhow::Result<csv::Writer<File>> {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(true)
        .delimiter(b'\t')
        .quote_style(csv::QuoteStyle::Always)
        .from_path(path)
        .context("failed to create csv writer")?;

    writer.write_record(headers).context("fai;ed to write csv header")?;

    Ok(writer)
}

impl CsvExporter {
    pub fn export_block(&mut self, block: Block) -> anyhow::Result<()> {
        self.export_transactions(block.transactions)?;
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
            self.transactions.write_record(row).context("failed to write csv transaction")?;
        }
        Ok(())
    }

    pub fn export_initial_accounts(&mut self, accounts: Vec<Account>) -> anyhow::Result<()> {
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
            self.accounts.write_record(row).context("failed to write csv transaction")?;
        }
        Ok(())
    }

    pub fn export_account_slots(&mut self, accounts: Vec<Account>) -> anyhow::Result<()> {
        for account in accounts {
            self.account_slots_id += 1;
            let row = [
                account.address.to_string(), // address
                account.bytecode.map(|x| x.to_string()).unwrap_or_default(), // bytecode
                account.balance.to_string(), // latest_balance
                account.nonce.to_string(),   // latest_nonce
                "0".to_owned(),              // previous_balance
                "0".to_owned(),              // previous_nonce
                "0".to_owned(),              // creation_block
            ];
            self.account_slots.write_record(row).context("failed to write csv account slots")?;
        }
        Ok(())
    }

    pub fn export_blocks(&mut self, blocks: Vec<BlockHeader>) -> anyhow::Result<()> {
        for block in blocks {
            self.blocks_id += 1;
            let row = [
                block.number.to_string(),           // number
                block.hash.to_string(),             // hash
                block.transactions_root.to_string(), // transactions_root
                block.gas_limit.to_string(),        // gas_limit
                block.gas_used.to_string(),         // gas_used
                block.bloom.to_string(),            // logs_bloom
                block.timestamp.to_string(),        // timestamp_in_secs
                block.parent_hash.to_string(),      // parent_hash
                block.author.to_string(),           // author
                block.extra_data.to_string(),       // extra_data
                block.miner.to_string(),            // miner
                block.difficulty.to_string(),       // difficulty
                block.receipts_root.to_string(),    // receipts_root
                block.uncle_hash.to_string(),       // uncle_hash
                block.size.to_string(),             // size
                block.state_root.to_string(),       // state_root
                block.total_difficulty.to_string(), // total_difficulty
                "0".to_owned(),                     // nonce
                now(),                              // created_at
            ];
            self.blocks.write_record(row).context("failed to write csv blocks")?;
        }
        Ok(())
    }

    pub fn export_historical_balances(&mut self, balances: Vec<(String, String, String)>) -> anyhow::Result<()> {
        self.historical_balances_id += 1;
        for (address, balance, block_number) in balances {
            let row = [
                address,      // address
                balance,      // balance
                block_number, // block_number
            ];
            self.historical_balances.write_record(row).context("failed to write csv historical balances")?;
        }
        Ok(())
    }

    pub fn export_historical_nonces(&mut self, nonces: Vec<(String, String, String)>) -> anyhow::Result<()> {
        self.historical_nonces_id += 1;
        for (address, nonce, block_number) in nonces {
            let row = [
                address,      // address
                nonce,        // nonce
                block_number, // block_number
            ];
            self.historical_nonces.write_record(row).context("failed to write csv historical nonces")?;
        }
        Ok(())
    }

    pub fn export_historical_slots(&mut self, slots: Vec<(String, String, String, String)>) -> anyhow::Result<()> {
        self.historical_slots_id += 1;
        for (idx, value, account_address, block_number) in slots {
            let row = [
                idx,            // idx
                value,          // value
                account_address, // account_address
                block_number,   // block_number
            ];
            self.historical_slots.write_record(row).context("failed to write csv historical slots")?;
        }
        Ok(())
    }

    pub fn export_logs(&mut self, logs: Vec<LogMined>) -> anyhow::Result<()> {
        self.logs_id += 1;
        for log in logs {
            let row = [
                log.address().to_string(),          // address
                log.log.data.to_string(),           // data
                log.transaction_hash.to_string(),   // transaction_hash
                log.transaction_index.to_string(),  // transaction_idx
                log.log_index.to_string(),          // log_idx
                log.block_number.to_string(),       // block_number
                log.block_hash.to_string(),         // block_hash
            ];
            self.logs.write_record(row).context("failed to write csv logs")?;
        }
        Ok(())
    }

    pub fn export_topics(&mut self, topics: Vec<(String, String, String, String, String, String, String)>) -> anyhow::Result<()> {
        self.topics_id += 1;
        for (topic, transaction_hash, transaction_idx, log_idx, topic_idx, block_number, block_hash) in topics {
            let row = [
                topic,            // topic
                transaction_hash, // transaction_hash
                transaction_idx,  // transaction_idx
                log_idx,          // log_idx
                topic_idx,        // topic_idx
                block_number,     // block_number
                block_hash,       // block_hash
            ];
            self.topics.write_record(row).context("failed to write csv topics")?;
        }
        Ok(())
    }


}

fn now() -> String {
    let now = chrono::Utc::now();
    now.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}
