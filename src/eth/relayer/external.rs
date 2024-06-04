use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use ethers_core::types::Transaction;
use futures::future::join_all;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::time::Instant;
use tracing::Span;

use super::transaction_dag::TransactionDag;
use crate::config::ExternalRelayerClientConfig;
use crate::config::ExternalRelayerServerConfig;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionMined;
use crate::ext::ResultExt;
use crate::ext::SpanExt;
use crate::infra::blockchain_client::pending_transaction::PendingTransaction;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::BlockchainClient;
use crate::log_and_err;

#[derive(Debug, thiserror::Error, derive_new::new)]
pub enum RelayError {
    #[error("Transaction Mismatch: {0}")]
    Mismatch(anyhow::Error),

    #[error("Compare Timeout: {0}")]
    CompareTimeout(anyhow::Error),
}

pub struct ExternalRelayer {
    pool: PgPool,

    /// RPC client that will submit transactions.
    substrate_chain: BlockchainClient,
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
            pool,
        })
    }

    /// Polls the next block to be relayed and relays it to Substrate.
    #[tracing::instrument(name = "external_relayer::relay_next_block", skip_all, fields(block_number))]
    pub async fn relay_next_block(&self) -> anyhow::Result<Option<BlockNumber>> {
        let Some(row) = sqlx::query!(
            r#"UPDATE relayer_blocks
                SET started = true
                WHERE number = (
                    SELECT MIN(number)
                    FROM relayer_blocks
                    WHERE finished = false
                )
                RETURNING payload"#
        )
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let block: Block = row.payload.try_into()?;
        let block_number = block.header.number;

        tracing::debug!(?block_number, "relaying block");

        // fill span
        let span = Span::current();
        span.rec("block_number", &block_number);

        // TODO: Replace failed transactions with transactions that will for sure fail in substrate (need access to primary keys)
        let dag = TransactionDag::new(block.transactions);

        #[cfg(feature = "metrics")]
        let start = metrics::now();

        if let Err(err) = self.relay_dag(dag).await {
            if let RelayError::CompareTimeout(_) = err {
                // This retries the entire block, but we could be retrying only each specific transaction that failed.
                // This is mostly a non-issue (except performance-wise)
                return Err(anyhow!("some receipt comparisons timed out, will retry next"));
            }

            tracing::warn!(?block_number, ?err, "some transactions mismatched");
            sqlx::query!(
                r#"UPDATE relayer_blocks
                    SET finished = true, mismatched = true
                    WHERE number = $1"#,
                block_number as _
            )
            .execute(&self.pool)
            .await?;
        } else {
            tracing::info!(?block_number, "block relayed with no mismatches");
            sqlx::query!(
                r#"UPDATE relayer_blocks
                    SET finished = true
                    WHERE number = $1"#,
                block_number as _
            )
            .execute(&self.pool)
            .await?;
        }

        #[cfg(feature = "metrics")]
        metrics::inc_relay_dag(start.elapsed());

        Ok(Some(block_number))
    }

    /// Compares the given receipt to the receipt returned by the pending transaction, retries until a receipt is returned
    /// to ensure the nonce was incremented. In case of a mismatch it returns an error describing what mismatched.
    #[tracing::instrument(name = "external_relayer::compare_receipt", skip_all, fields(hash))]
    async fn compare_receipt(&self, stratus_receipt: ExternalReceipt, substrate_pending_transaction: PendingTransaction<'_>) -> anyhow::Result<(), RelayError> {
        let tx_hash: Hash = stratus_receipt.0.transaction_hash.into();
        tracing::info!(?tx_hash, "comparing receipts");

        // fill span
        let span = Span::current();
        span.rec("hash", &tx_hash);

        let start = Instant::now();
        let mut substrate_receipt = substrate_pending_transaction;
        loop {
            match substrate_receipt.await {
                Ok(Some(substrate_receipt)) =>
                    if let Err(compare_error) = substrate_receipt.compare(&stratus_receipt) {
                        let err_string = compare_error.to_string();
                        let error = log_and_err!("transaction mismatch!").context(err_string.clone());
                        self.save_mismatch(stratus_receipt, substrate_receipt, &err_string).await;
                        return error.map_err(RelayError::Mismatch);
                    } else {
                        return Ok(());
                    },
                Ok(None) =>
                    if start.elapsed().as_secs() <= 30 {
                        tracing::warn!(?tx_hash, "no receipt returned by substrate, retrying...");
                    } else {
                        tracing::error!(?tx_hash, "no receipt returned by substrate for more than 30 seconds, retrying block");
                        return Err(RelayError::CompareTimeout(anyhow!("no receipt returned by substrate for more than 30 seconds")));
                    },
                Err(error) => {
                    tracing::error!(?tx_hash, ?error, "failed to fetch substrate receipt, retrying...");
                }
            }
            substrate_receipt = PendingTransaction::new(tx_hash, &self.substrate_chain);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Save a transaction mismatch to postgres, if it fails, save it to a file.
    #[tracing::instrument(name = "external_relayer::save_mismatch", skip_all)]
    async fn save_mismatch(&self, stratus_receipt: ExternalReceipt, substrate_receipt: ExternalReceipt, err_string: &str) {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let hash = stratus_receipt.hash();
        let block_number = stratus_receipt.block_number.map(|inner| inner.as_u64() as i64); // could panic if block number as huge for some reason

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
                let mut file = File::create(format!("data/mismatched_transactions/{}.json", hash))
                    .await
                    .expect("opening the file should not fail");
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
            Ok(res) =>
                if res.rows_affected() == 0 {
                    tracing::info!(
                        ?block_number,
                        ?hash,
                        "transaction mismatch already in database (this should only happen if this block is being retried)."
                    );
                    return;
                },
        }

        #[cfg(feature = "metrics")]
        metrics::inc_save_mismatch(start.elapsed());
    }

    /// Relays a transaction to Substrate and waits until the transaction is in the mempool by
    /// calling eth_getTransactionByHash. (infallible)
    #[tracing::instrument(name = "external_relayer::relay_and_check_mempool", skip_all, fields(hash))]
    pub async fn relay_and_check_mempool(&self, tx_mined: TransactionMined) -> (PendingTransaction, ExternalReceipt) {
        let tx_hash = tx_mined.input.hash;
        tracing::debug!(?tx_hash, "relaying transaction");

        // fill span
        let span = Span::current();
        span.rec("hash", &tx_hash);

        let ethers_tx = Transaction::from(tx_mined.input.clone());
        let tx = loop {
            match self.substrate_chain.send_raw_transaction(tx_hash, ethers_tx.rlp()).await {
                Ok(tx) => break tx,
                Err(err) => {
                    tracing::debug!(
                        ?tx_hash,
                        "substrate_chain.send_raw_transaction returned an error, checking if transaction was sent anyway"
                    );
                    if self.substrate_chain.fetch_transaction(tx_hash).await.unwrap_or(None).is_some() {
                        tracing::debug!(?tx_hash, "transaction found on substrate");
                        return (PendingTransaction::new(tx_hash, &self.substrate_chain), ExternalReceipt(tx_mined.into()));
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
            tokio::time::sleep(Duration::from_millis(100)).await;
            tries += 1;
        }

        (tx, ExternalReceipt(tx_mined.into()))
    }

    /// Relays a dag by removing its roots and sending them consecutively. Returns `Ok` if we confirmed that all transactions
    /// had the same receipts, returns `Err` if one or more transactions had receipts mismatches. The mismatches are saved
    /// on the `mismatches` table in pgsql, or in data/mismatched_transactions as a fallback.
    #[tracing::instrument(name = "external_relayer::relay_dag", skip_all)]
    async fn relay_dag(&self, mut dag: TransactionDag) -> anyhow::Result<(), RelayError> {
        tracing::debug!("relaying transactions");
        let mut results = vec![];
        while let Some(roots) = dag.take_roots() {
            let futures = roots.into_iter().map(|root_tx| self.relay_and_check_mempool(root_tx));
            results.extend(join_all(futures).await);
        }

        let futures = results
            .into_iter()
            .map(|(substrate_pending_tx, stratus_receipt)| self.compare_receipt(stratus_receipt, substrate_pending_tx));

        if join_all(futures)
            .await
            .into_iter()
            .filter_map(Result::err)
            .any(|err| matches!(err, RelayError::CompareTimeout(_)))
        {
            return Err(RelayError::CompareTimeout(anyhow!("some comparisons timed out, should retry them.")));
        }

        Ok(())
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
    pub async fn send_to_relayer(&self, block: Block) -> anyhow::Result<()> {
        let block_number = block.header.number;
        tracing::debug!(?block_number, "sending block to relayer");
        
        tracing::debug!("#################################");
        for transaction in block.transactions.iter() {
            tracing::debug!(?transaction, "transaction")
        }
        tracing::debug!("#################################");

        // fill span
        let span = Span::current();
        span.rec("block_number", &block_number);

        sqlx::query!(
            "INSERT INTO relayer_blocks (number, payload) VALUES ($1, $2)",
            block_number as _,
            serde_json::to_value(block)?
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
