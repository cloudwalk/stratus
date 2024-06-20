use std::collections::HashSet;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use ethers_core::types::Transaction;
use futures::future::join_all;
use futures::StreamExt;
use itertools::Itertools;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::time::timeout;
use tokio::time::Instant;
use tracing::Span;

use super::transaction_dag::TransactionDag;
use crate::config::ExternalRelayerClientConfig;
use crate::config::ExternalRelayerServerConfig;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionValueChange;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::ext::traced_sleep;
use crate::ext::ResultExt;
use crate::ext::SleepReason;
use crate::ext::SpanExt;
use crate::infra::blockchain_client::pending_transaction::PendingTransaction;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::metrics::inc_compare_final_state;
use crate::infra::BlockchainClient;
use crate::log_and_err;

type MismatchedBlocks = HashSet<BlockNumber>;
type TimedoutBlocks = HashSet<BlockNumber>;

#[derive(Debug, thiserror::Error, derive_new::new)]
pub enum RelayError {
    #[error("Transaction Mismatch: {1}")]
    Mismatch(BlockNumber, anyhow::Error),

    #[error("Compare Timeout: {1}")]
    CompareTimeout(BlockNumber, anyhow::Error),
}

pub struct ExternalRelayer {
    pool: PgPool,

    /// RPC client that will submit transactions.
    substrate_chain: BlockchainClient,

    /// RPC client that will submit transactions.
    stratus_chain: BlockchainClient,
}

impl ExternalRelayer {
    /// Creates a new [`ExternalRelayer`].
    pub async fn new(config: ExternalRelayerServerConfig) -> anyhow::Result<Self> {
        tracing::info!(?config, "creating external relayer");
        let pool = PgPoolOptions::new()
            .min_connections(config.connections)
            .max_connections(config.connections)
            .acquire_timeout(config.acquire_timeout)
            .connect(&config.url)
            .await
            .expect("should not fail to create pgpool");

        Ok(Self {
            substrate_chain: BlockchainClient::new_http(&config.forward_to, config.rpc_timeout).await?,
            stratus_chain: BlockchainClient::new_http(&config.stratus_rpc, config.rpc_timeout).await?,
            pool,
        })
    }

    fn combine_transactions(blocks: Vec<Block>) -> Vec<TransactionMined> {
        blocks.into_iter().flat_map(|block| block.transactions).collect()
    }

    async fn blocks_have_been_mined(&self, blocks: Vec<Hash>) -> bool {
        let futures = blocks.into_iter().map(|hash| self.stratus_chain.fetch_block_by_hash(hash, false));
        !join_all(futures).await.into_iter().any(|result| result.is_err() || result.unwrap().is_null())
    }

    #[tracing::instrument(name = "external_relayer::relay_next_block", skip_all)]
    async fn compare_final_state(&self, changed_slots: HashSet<(Address, SlotIndex)>, block_number: BlockNumber) {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let point_in_time = StoragePointInTime::Past(block_number);
        let mut futures = vec![];
        for (addr, index) in changed_slots {
            futures.push(async move {
            let stratus_slot_value = loop {
                match self.stratus_chain.fetch_storage_at(&addr, &index, point_in_time).await {
                    Ok(value) => break value,
                    Err(err) => tracing::warn!(?addr, ?index, ?err, "failed to fetch slot value from stratus, retrying..."),
                }
            };

            let substrate_slot_value = loop {
                match self.substrate_chain.fetch_storage_at(&addr, &index, StoragePointInTime::Present).await {
                    Ok(value) => break value,
                    Err(err) => tracing::warn!(?addr, ?index, ?err, "failed to fetch slot value from substrate, retrying..."),
                }
            };

            if stratus_slot_value != substrate_slot_value {
                tracing::error!(?addr, ?index, ?point_in_time, "evm state mismatch between stratus and substrate");
                while let Err(e) = sqlx::query!(
                    "INSERT INTO slot_mismatches (address, index, block_number, stratus_value, substrate_value) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
                    addr as _,
                    index as _,
                    block_number as _,
                    stratus_slot_value as _,
                    substrate_slot_value as _
                )
                .execute(&self.pool)
                .await {
                    tracing::warn!(?e, "failed to insert slot mismatch, retrying...")
                }
            }})
        }

        let mut buffer = futures::stream::iter(futures).buffer_unordered(100);
        while let Some(_) = buffer.next().await {}

        #[cfg(feature = "metrics")]
        inc_compare_final_state(start.elapsed());
    }

    /// Polls the next block to be relayed and relays it to Substrate.
    #[tracing::instrument(name = "external_relayer::relay_next_block", skip_all, fields(block_number))]
    pub async fn relay_blocks(&self) -> anyhow::Result<Vec<BlockNumber>> {
        let block_rows = sqlx::query!(
            r#"
            WITH cte AS (
                SELECT number
                FROM relayer_blocks
                WHERE finished = false
                ORDER BY number ASC
                LIMIT 2
            )
            UPDATE relayer_blocks r
                SET started = true
                FROM cte
                WHERE r.number = cte.number
                RETURNING r.number, r.payload"#
        )
        .fetch_all(&self.pool)
        .await?;

        if block_rows.len() == 0 {
            tracing::info!("no blocks to relay");
            return Ok(vec![]);
        }

        let block_numbers: HashSet<BlockNumber> = block_rows.iter().map(|row| row.number.into()).collect();
        let max_number = block_numbers.iter().max().cloned().unwrap();

        // fill span
        Span::with(|s| s.rec_str("block_number", &max_number));

        let blocks: Vec<Block> = block_rows
            .into_iter()
            .sorted_by_key(|row| row.number)
            .map(|row| row.payload.try_into())
            .collect::<Result<_, _>>()?;

        if !self.blocks_have_been_mined(blocks.iter().map(|block| block.hash()).collect()).await {
            return Err(anyhow!("some blocks in this batch have not been mined in stratus"));
        }

        let combined_transactions = Self::combine_transactions(blocks);
        let modified_slots = TransactionDag::get_slot_writes(&combined_transactions);

        // TODO: Replace failed transactions with transactions that will for sure fail in substrate (need access to primary keys)
        let dag = TransactionDag::new(combined_transactions);
        let (mismatched_blocks, timedout_blocks) = self.relay_dag(dag).await;

        let non_ok_blocks: HashSet<BlockNumber> = mismatched_blocks.union(&timedout_blocks).cloned().collect();

        let only_mismatched_blocks: Vec<BlockNumber> = mismatched_blocks.difference(&timedout_blocks).cloned().collect();
        let ok_blocks: Vec<BlockNumber> = block_numbers.difference(&non_ok_blocks).cloned().collect();

        if !timedout_blocks.is_empty() {
            tracing::warn!(?timedout_blocks, "some blocks timed-out");
        }

        if !only_mismatched_blocks.is_empty() {
            tracing::warn!(?only_mismatched_blocks, "some transactions mismatched");

            sqlx::query!(
                r#"UPDATE relayer_blocks
                SET finished = true, mismatched = true
                WHERE number = ANY($1)"#,
                &only_mismatched_blocks[..] as _
            )
            .execute(&self.pool)
            .await?;
        }

        if !ok_blocks.is_empty() {
            sqlx::query!(
                r#"UPDATE relayer_blocks
                    SET finished = true
                    WHERE number = ANY($1)"#,
                &ok_blocks[..] as _
            )
            .execute(&self.pool)
            .await?;
        }

        self.compare_final_state(modified_slots, max_number).await;
        Ok(ok_blocks.into_iter().chain(only_mismatched_blocks.into_iter()).collect())
    }

    /// Compares the given receipt to the receipt returned by the pending transaction, retries until a receipt is returned
    /// to ensure the nonce was incremented. In case of a mismatch it returns an error describing what mismatched.
    #[tracing::instrument(name = "external_relayer::compare_receipt", skip_all, fields(hash))]
    async fn compare_receipt(&self, mut stratus_tx: TransactionMined, substrate_pending_transaction: PendingTransaction<'_>) -> anyhow::Result<(), RelayError> {
        #[cfg(feature = "metrics")]
        let start_metric = metrics::now();

        let tx_hash: Hash = stratus_tx.input.hash;
        let block_number: BlockNumber = stratus_tx.block_number;

        tracing::info!(?block_number, ?tx_hash, "comparing receipts");

        // fill span
        Span::with(|s| s.rec_str("hash", &tx_hash));

        let start = Instant::now();
        let mut substrate_receipt = substrate_pending_transaction;
        let _res = loop {
            let Ok(receipt) = timeout(Duration::from_secs(30), substrate_receipt).await else {
                tracing::error!(
                    ?block_number,
                    ?tx_hash,
                    "no receipt returned by substrate for more than 30 seconds, retrying block"
                );
                break Err(RelayError::CompareTimeout(
                    block_number,
                    anyhow!("no receipt returned by substrate for more than 30 seconds"),
                ));
            };

            match receipt {
                Ok(Some(substrate_receipt)) => {
                    let _ = stratus_tx.execution.apply_receipt(&substrate_receipt);
                    if let Err(compare_error) = stratus_tx.execution.compare_with_receipt(&substrate_receipt) {
                        let err_string = compare_error.to_string();
                        let error = log_and_err!("transaction mismatch!").context(err_string.clone());
                        self.save_mismatch(stratus_tx, substrate_receipt, &err_string).await;
                        break error.map_err(|err| RelayError::Mismatch(block_number, err));
                    } else {
                        break Ok(());
                    }
                }
                Ok(None) => {
                    if start.elapsed().as_secs() <= 30 {
                        tracing::warn!(?tx_hash, "no receipt returned by substrate, retrying...");
                    } else {
                        tracing::error!(?tx_hash, "no receipt returned by substrate for more than 30 seconds, retrying block");
                        break Err(RelayError::CompareTimeout(
                            block_number,
                            anyhow!("no receipt returned by substrate for more than 30 seconds"),
                        ));
                    }
                }
                Err(error) => {
                    tracing::error!(?tx_hash, ?error, "failed to fetch substrate receipt, retrying...");
                }
            }
            substrate_receipt = PendingTransaction::new(tx_hash, &self.substrate_chain);
            traced_sleep(Duration::from_millis(50), SleepReason::SyncData).await;
        };

        #[cfg(feature = "metrics")]
        metrics::inc_compare_receipts(start_metric.elapsed());

        _res
    }

    /// Save a transaction mismatch to postgres, if it fails, save it to a file.
    #[tracing::instrument(name = "external_relayer::save_mismatch", skip_all)]
    async fn save_mismatch(&self, stratus_receipt: TransactionMined, substrate_receipt: ExternalReceipt, err_string: &str) {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let hash = stratus_receipt.input.hash;
        let block_number = stratus_receipt.block_number;

        tracing::info!(?block_number, ?hash, "saving transaction mismatch");

        let stratus_json = serde_json::to_value(stratus_receipt).expect_infallible();
        let substrate_json = serde_json::to_value(substrate_receipt).expect_infallible();
        let res = sqlx::query!(
            "INSERT INTO mismatches (hash, block_number, stratus_receipt, substrate_receipt, error) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
            &hash as _,
            block_number as _,
            &stratus_json,
            &substrate_json,
            err_string
        )
        .execute(&self.pool)
        .await;

        match res {
            Err(err) => {
                tracing::error!(?block_number, ?hash, "failed to insert row in pgsql, saving mismatche to json");
                let mut file = File::create(format!("data/{}.json", hash)).await.expect("opening the file should not fail");
                let json = serde_json::json!(
                    {
                        "stratus_receipt": stratus_json,
                        "substrate_receipt": substrate_json,
                    }
                );
                file.write_all(json.to_string().as_bytes())
                    .await
                    .expect("writing the mismatch to a file should not fail");
                tracing::error!(?err, "failed to save mismatch, saving to file");
            }
            Ok(res) => {
                if res.rows_affected() == 0 {
                    tracing::info!(
                        ?block_number,
                        ?hash,
                        "transaction mismatch already in database (this should only happen if this block is being retried)."
                    );
                    return;
                }
            }
        }

        #[cfg(feature = "metrics")]
        metrics::inc_save_mismatch(start.elapsed());
    }

    /// Relays a transaction to Substrate and waits until the transaction is in the mempool by
    /// calling eth_getTransactionByHash. (infallible)
    #[tracing::instrument(name = "external_relayer::relay_and_check_mempool", skip_all, fields(hash))]
    pub async fn relay_and_check_mempool(&self, tx_mined: TransactionMined) -> (PendingTransaction, TransactionMined) {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let tx_hash = tx_mined.input.hash;
        tracing::info!(?tx_hash, "relaying transaction");

        // fill span
        Span::with(|s| s.rec_str("hash", &tx_hash));

        let ethers_tx = Transaction::from(tx_mined.input.clone());
        let tx = loop {
            match self.substrate_chain.send_raw_transaction(tx_hash, ethers_tx.rlp()).await {
                Ok(tx) => break tx,
                Err(err) => {
                    tracing::info!(
                        ?tx_hash,
                        "substrate_chain.send_raw_transaction returned an error, checking if transaction was sent anyway"
                    );
                    if self.substrate_chain.fetch_transaction(tx_hash).await.unwrap_or(None).is_some() {
                        tracing::info!(?tx_hash, "transaction found on substrate");
                        return (PendingTransaction::new(tx_hash, &self.substrate_chain), tx_mined);
                    }
                    tracing::warn!(?tx_hash, ?err, "failed to send raw transaction, retrying...");
                    continue;
                }
            }
        };

        // this is probably redundant since send_raw_transaction probably only succeeds if the transaction was added to the mempool already.
        tracing::info!(?tx_mined.input.hash, "polling eth_getTransactionByHash");
        let mut tries = 0;
        while self.substrate_chain.fetch_transaction(tx_mined.input.hash).await.unwrap_or(None).is_none() {
            tracing::warn!(?tx_mined.input.hash, ?tries, "transaction not found, retrying...");
            traced_sleep(Duration::from_millis(100), SleepReason::SyncData).await;
            tries += 1;
        }

        #[cfg(feature = "metrics")]
        metrics::inc_relay_and_check_mempool(start.elapsed());

        (tx, tx_mined)
    }

    /// Relays a dag by removing its roots and sending them consecutively. Returns `Ok` if we confirmed that all transactions
    /// had the same receipts, returns `Err` if one or more transactions had receipts mismatches. The mismatches are saved
    /// on the `mismatches` table in pgsql, or in ./data as a fallback.
    #[tracing::instrument(name = "external_relayer::relay_dag", skip_all)]
    async fn relay_dag(&self, mut dag: TransactionDag) -> (MismatchedBlocks, TimedoutBlocks) {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        tracing::debug!("relaying transactions");

        let mut results = vec![];
        while let Some(roots) = dag.take_roots() {
            let futures = roots.into_iter().map(|root_tx| self.relay_and_check_mempool(root_tx));
            results.extend(join_all(futures).await);
        }

        let futures = results
            .into_iter()
            .map(|(substrate_pending_tx, stratus_receipt)| self.compare_receipt(stratus_receipt, substrate_pending_tx));

        let errors = join_all(futures).await.into_iter().filter_map(Result::err);

        let mut mismatched_blocks: MismatchedBlocks = HashSet::new();
        let mut timedout_blocks: TimedoutBlocks = HashSet::new();

        for error in errors {
            match error {
                RelayError::CompareTimeout(number, _) => timedout_blocks.insert(number),
                RelayError::Mismatch(number, _) => mismatched_blocks.insert(number),
            };
        }

        #[cfg(feature = "metrics")]
        metrics::inc_relay_dag(start.elapsed());

        (mismatched_blocks, timedout_blocks)
    }
}

pub struct ExternalRelayerClient {
    pool: PgPool,
}

impl ExternalRelayerClient {
    /// Creates a new [`ExternalRelayerClient`].
    pub async fn new(config: ExternalRelayerClientConfig) -> Self {
        tracing::info!(?config, "creating external relayer client");
        let storage = PgPoolOptions::new()
            .min_connections(config.connections)
            .max_connections(config.connections)
            .acquire_timeout(config.acquire_timeout)
            .connect(&config.url)
            .await
            .expect("should not fail to create pgpool");

        Self { pool: storage }
    }

    /// Insert the block into the relayer_blocks table on pgsql to be processed by the relayer. Returns Err if
    /// the insertion fails.
    #[tracing::instrument(name = "external_relayer_client::send_to_relayer", skip_all, fields(block_number))]
    pub async fn send_to_relayer(&self, mut block: Block) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let block_number = block.header.number;
        tracing::info!(?block_number, "sending block to relayer");

        // strip bytecode
        for tx in block.transactions.iter_mut() {
            for (_, change) in tx.execution.changes.iter_mut() {
                change.bytecode = ExecutionValueChange::default();
            }
        }

        let block_json = serde_json::to_value(block)?;
        // fill span
        Span::with(|s| s.rec_str("block_number", &block_number));
        let mut remaining_tries = 5;

        while remaining_tries > 0 {
            if let Err(err) = sqlx::query!(
                r#"
                    INSERT INTO relayer_blocks
                    (number, payload)
                    VALUES ($1, $2)
                    ON CONFLICT (number) DO UPDATE
                    SET payload = EXCLUDED.payload"#,
                block_number as _,
                &block_json
            )
            .execute(&self.pool)
            .await
            {
                remaining_tries -= 1;
                tracing::warn!(?err, ?remaining_tries, "failed to insert into relayer_blocks");
            } else {
                break;
            }
        }

        #[cfg(feature = "metrics")]
        metrics::inc_send_to_relayer(start.elapsed());

        match remaining_tries {
            0 => Err(anyhow!("failed to insert block into relayer_blocks after 5 tries")),
            _ => Ok(()),
        }
    }
}
