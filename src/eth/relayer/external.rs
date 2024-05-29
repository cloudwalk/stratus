use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use ethers_core::types::Transaction;
use futures::future::join_all;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use super::transaction_dag::TransactionDag;
use crate::config::ExternalRelayerClientConfig;
use crate::config::ExternalRelayerServerConfig;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::TransactionMined;
use crate::ext::ResultExt;
use crate::infra::blockchain_client::pending_transaction::PendingTransaction;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::BlockchainClient;
use crate::log_and_err;

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
    #[tracing::instrument(skip_all)]
    pub async fn relay_next_block(&self) -> anyhow::Result<Option<BlockNumber>> {
        let Some(row) = sqlx::query!(
            r#"UPDATE relayer_blocks
                SET relayed = true
                WHERE number = (
                    SELECT MIN(number)
                    FROM relayer_blocks
                    WHERE relayed = false
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

        // TODO: Replace failed transactions with transactions that will for sure fail in substrate (need access to primary keys)
        let dag = TransactionDag::new(block.transactions);

        #[cfg(feature = "metrics")]
        let start = metrics::now();

        self.relay_dag(dag).await?; // if Ok mark block as sucess if Err mark it as failed or smth like that

        #[cfg(feature = "metrics")]
        metrics::inc_relay_dag(start.elapsed());

        Ok(Some(block_number))
    }

    async fn compare_receipt(&self, stratus_receipt: ExternalReceipt, substrate_receipt: PendingTransaction<'_>) -> anyhow::Result<()> {
        match substrate_receipt.await {
            Ok(Some(substrate_receipt)) =>
                if let Err(compare_error) = substrate_receipt.compare(&stratus_receipt) {
                    let err_string = compare_error.to_string();
                    let error = log_and_err!("transaction mismatch!").context(err_string.clone());
                    self.save_mismatch(stratus_receipt, Some(substrate_receipt), &err_string).await;
                    error
                } else {
                    Ok(())
                },
            Ok(None) => {
                self.save_mismatch(stratus_receipt, None, "no receipt returned by substrate").await;
                Err(anyhow!("no receipt returned by substrate"))
            }
            Err(error) => {
                let error = error.context("failed to fetch substrate receipt");
                let err_str = error.to_string();
                self.save_mismatch(stratus_receipt, None, &err_str.to_string()).await;
                Err(error)
            }
        }
    }

    /// Save a transaction mismatch to postgres, if it fails, save it to a file.
    async fn save_mismatch(&self, stratus_receipt: ExternalReceipt, substrate_receipt: Option<ExternalReceipt>, err_string: &str) {
        let hash = stratus_receipt.hash().to_string();
        let stratus_json = serde_json::to_value(stratus_receipt).expect_infallible();
        let substrate_json = serde_json::to_value(substrate_receipt).expect_infallible();
        let res = sqlx::query!(
            "INSERT INTO mismatches (hash, stratus_receipt, substrate_receipt, error) VALUES ($1, $2, $3, $4)",
            &hash,
            &stratus_json,
            &substrate_json,
            err_string
        )
        .execute(&self.pool)
        .await; // should fallback is case of error?
        if let Err(err) = res {
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
    }

    /// Relays a transaction to Substrate and waits until the transaction is in the mempool by
    /// calling eth_getTransactionByHash.
    #[tracing::instrument(skip_all)]
    pub async fn relay_and_check_mempool(&self, tx_mined: TransactionMined) -> anyhow::Result<(PendingTransaction, ExternalReceipt)> {
        tracing::debug!(?tx_mined.input.hash, "relaying transaction");

        let ethers_tx = Transaction::from(tx_mined.input.clone());
        let tx = self.substrate_chain.send_raw_transaction(tx_mined.input.hash, ethers_tx.rlp()).await?;

        tracing::debug!(?tx_mined.input.hash, "polling eth_getTransactionByHash");
        let mut tries = 0;
        while self.substrate_chain.fetch_transaction(tx_mined.input.hash).await?.is_none() {
            if tries > 50 {
                return Err(anyhow!("transaction was not found in mempool after 500ms"));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
            tries += 1;
        }

        Ok((tx, ExternalReceipt(tx_mined.into())))
    }

    pub async fn relay(&self, tx_mined: TransactionMined) -> anyhow::Result<(PendingTransaction, ExternalReceipt)> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let res = match self.relay_and_check_mempool(tx_mined.clone()).await {
            Err(err) => {
                let err = err.context("relay and check mempool failed");
                let err_string = err.to_string();
                self.save_mismatch(ExternalReceipt(tx_mined.into()), None, &err_string).await;
                Err(err)
            }
            ok => ok,
        };

        #[cfg(feature = "metrics")]
        metrics::inc_relay_and_check_mempool(start.elapsed());

        res
    }

    #[tracing::instrument(skip_all)]
    async fn relay_dag(&self, mut dag: TransactionDag) -> anyhow::Result<()> {
        tracing::debug!("relaying transactions");
        let mut tx_failed = false;

        let mut results = vec![];
        while let Some(roots) = dag.take_roots() {
            let futures = roots.into_iter().map(|root_tx| self.relay(root_tx));
            for result in join_all(futures).await {
                match result {
                    Ok(res) => results.push(res),
                    Err(err) => {
                        tracing::error!(?err);
                        tx_failed = true;
                    }
                }
            }
        }

        let futures = results
            .into_iter()
            .map(|(substrate_pending_tx, stratus_receipt)| self.compare_receipt(stratus_receipt, substrate_pending_tx));

        join_all(futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .context("some transactions failed")?;

        if tx_failed {
            return Err(anyhow!("some transactions failed"));
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

    pub async fn send_to_relayer(&self, block: Block) -> anyhow::Result<()> {
        tracing::debug!(?block.header.number, "sending block to relayer");
        let block_number = block.header.number;
        sqlx::query!("INSERT INTO relayer_blocks VALUES ($1, $2)", block_number as _, serde_json::to_value(block)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
