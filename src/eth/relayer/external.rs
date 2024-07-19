use std::collections::HashSet;
use std::fs::create_dir;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Context;
use ethers_core::types::transaction::eip2718::TypedTransaction;
use ethers_core::types::Bytes;
use ethers_core::types::Transaction;
use ethers_core::types::TransactionRequest;
use ethers_signers::LocalWallet;
use ethers_signers::Signer;
use futures::future::join_all;
use futures::StreamExt;
use itertools::Itertools;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::Span;

use super::transaction_dag::TransactionDag;
use super::SIGNATURES;
use super::UNRESIGNABLE_FUNCTIONS;
use crate::config::ExternalRelayerClientConfig;
use crate::config::ExternalRelayerServerConfig;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionValueChange;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::StoragePointInTime;
use crate::ext::to_json_value;
use crate::ext::JsonValue;
use crate::infra::blockchain_client::pending_transaction::PendingTransaction;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::metrics::inc_compare_final_state;
use crate::infra::tracing::SpanExt;
use crate::infra::BlockchainClient;
use crate::log_and_err;

type MismatchedBlocks = HashSet<BlockNumber>;

#[derive(Debug, thiserror::Error, derive_new::new)]
pub enum RelayError {
    #[error("Transaction Mismatch: {1}")]
    Mismatch(BlockNumber, anyhow::Error),

    #[error("Transaction not found")]
    TransactionNotFound,

    #[error("Relaying transaction timed-out")]
    RelayTimeout,
}

struct TxSigner {
    wallet: LocalWallet,
    nonce: Nonce,
    address: Address,
}

impl TxSigner {
    pub async fn new(private_key: String, chain: &BlockchainClient) -> anyhow::Result<Self> {
        let private_key = const_hex::decode(private_key)?;
        let wallet = LocalWallet::from_bytes(&private_key)?;
        let address = wallet.address().into();
        let nonce = chain.fetch_transaction_count(&address).await?;
        Ok(Self { wallet, nonce, address })
    }

    pub async fn sync_nonce(&mut self, chain: &BlockchainClient) -> anyhow::Result<()> {
        self.nonce = chain.fetch_transaction_count(&self.wallet.address().into()).await?;
        Ok(())
    }

    pub fn sign_transaction_input(&mut self, mut tx_input: TransactionInput) -> TransactionInput {
        tracing::info!(?tx_input.hash, "signing transaction");
        let gas_limit = 9_999_999u32;
        let tx: TransactionRequest = <TransactionRequest as From<TransactionInput>>::from(tx_input.clone())
            .nonce(self.nonce)
            .gas(gas_limit);

        let req = TypedTransaction::Legacy(tx);
        let signature = self.wallet.sign_transaction_sync(&req).unwrap();
        let new_hash = req.hash(&signature);

        tx_input.signer = self.wallet.address().into();
        tx_input.gas_limit = gas_limit.into();
        // None is Legacy
        tx_input.tx_type = None;
        tx_input.hash = new_hash.into();
        tx_input.nonce = self.nonce;
        tx_input.r = signature.r;
        tx_input.s = signature.s;
        tx_input.v = signature.v.into();

        self.nonce = self.nonce.next();
        tx_input
    }
}

pub struct ExternalRelayer {
    pool: PgPool,

    /// RPC client that will submit transactions.
    substrate_chain: BlockchainClient,

    /// RPC client that will submit transactions.
    stratus_chain: BlockchainClient,

    /// The signer to resign whatever transactions we can
    signer: TxSigner,

    /// How many blocks to fetch per relay operation
    blocks_to_fetch: u64,

    /// Wallets that have mismatched nonces
    out_of_sync_wallets: HashSet<Address>,
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

        let substrate_chain = BlockchainClient::new_http(&config.forward_to, config.rpc_timeout).await?;
        let signer = TxSigner::new(config.signer, &substrate_chain).await?;
        let mut relayer = Self {
            substrate_chain,
            stratus_chain: BlockchainClient::new_http(&config.stratus_rpc, config.rpc_timeout).await?,
            pool,
            signer,
            blocks_to_fetch: config.blocks_to_fetch,
            out_of_sync_wallets: HashSet::new(),
        };
        relayer.load_out_of_sync_wallets().await?;
        if config.cleanup {
            relayer.cleanup_database().await;
        }
        Ok(relayer)
    }

    async fn insert_unsent_transaction(&mut self, tx_mined: TransactionMined) {
        let tx_hash = tx_mined.input.hash;
        let tx_json = to_json_value(tx_mined);
        while let Err(e) = sqlx::query!(
            "INSERT INTO unsent_transactions(transaction_hash, transaction) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            tx_hash as _,
            tx_json as _
        )
        .execute(&self.pool)
        .await
        {
            tracing::warn!(?e, "failed to insert unsent transaction, retrying...");
        }
    }

    async fn insert_out_of_sync_wallet(&mut self, address: Address) {
        self.out_of_sync_wallets.insert(address);
        while let Err(e) = sqlx::query!("INSERT INTO out_of_sync_wallets(address) VALUES ($1) ON CONFLICT DO NOTHING", address as _)
            .execute(&self.pool)
            .await
        {
            tracing::warn!(?e, "failed to insert out of sync wallet, retrying...");
        }
    }

    async fn combine_transactions(&mut self, blocks: Vec<Block>) -> anyhow::Result<Vec<TransactionMined>> {
        let mut combined_transactions = vec![];
        for mut tx in blocks.into_iter().flat_map(|block| block.transactions).sorted() {
            let should_resign = {
                tx.input.extract_function().is_some_and(|sig| {
                    let scope = sig.split_once("::").map(|(scope, _)| scope);
                    let function_split = sig.split_once('(');
                    let contract_is_resignable = || SIGNATURES.contains(sig.as_ref()) || scope.is_some_and(|scope| SIGNATURES.contains(scope));
                    let function_is_not_blocklisted = || !function_split.is_some_and(|(function, _)| UNRESIGNABLE_FUNCTIONS.contains(function));
                    contract_is_resignable() && function_is_not_blocklisted()
                })
            };
            if should_resign {
                let transaction_signed = self.get_mapped_transaction(tx.input.hash).await?;
                if let Some(transaction) = transaction_signed {
                    tx.input = transaction;
                } else if tx.is_success() {
                    if !self.out_of_sync_wallets.contains(&tx.input.signer) {
                        self.insert_out_of_sync_wallet(tx.input.signer).await;
                    }
                    let prev_hash = tx.input.hash;
                    tx.input = self.signer.sign_transaction_input(tx.input);
                    self.insert_transaction_mapping(prev_hash, &tx.input).await;
                } else {
                    continue;
                }
            } else if self.out_of_sync_wallets.contains(&tx.input.signer) {
                self.insert_unsent_transaction(tx).await;
                continue;
            }
            combined_transactions.push(tx);
        }
        Ok(combined_transactions)
    }

    async fn load_out_of_sync_wallets(&mut self) -> anyhow::Result<()> {
        self.out_of_sync_wallets = sqlx::query!("SELECT address FROM out_of_sync_wallets")
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|row| Address::try_from(row.address))
            .collect::<Result<HashSet<_>, _>>()?;
        Ok(())
    }

    /// Cleanups the database in case of rollbacks by deleting blocks in the relayer_blocks table that have not been mined in stratus
    async fn cleanup_database(&self) {
        tracing::info!("starting db cleanup");
        loop {
            let Ok(blocks) = self.fetch_blocks(200).await else { continue };
            let Ok(missing) = self.missing_blocks(blocks.into_iter().map(|block| block.hash()).collect()).await else {
                continue;
            };
            if missing.is_empty() {
                break;
            }
            tracing::info!(?missing, "found missing blocks, cleaning up");
            let missing_json = missing.into_iter().map(to_json_value).collect_vec();
            while sqlx::query!(
                "DELETE FROM relayer_blocks WHERE payload->'header'->'hash' IN (SELECT * FROM UNNEST($1::JSONB[]))",
                missing_json as _
            )
            .execute(&self.pool)
            .await
            .is_err()
            {}
        }
        tracing::info!("finished db cleanup");
    }

    async fn missing_blocks(&self, blocks: Vec<Hash>) -> anyhow::Result<Vec<Hash>> {
        let futures = blocks.iter().map(|hash| self.stratus_chain.fetch_block_by_hash(*hash, false));
        Ok(join_all(futures)
            .await
            .into_iter()
            .collect::<Result<Vec<JsonValue>, _>>()?
            .into_iter()
            .map(|inner| inner.is_null())
            .zip(blocks)
            .filter_map(|(is_missing, block)| if is_missing { Some(block) } else { None })
            .collect())
    }

    #[tracing::instrument(name = "external_relayer::relay_next_block", skip_all)]
    async fn compare_final_state(&self, changed_slots: HashSet<(Address, SlotIndex)>, block_number: BlockNumber) {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let point_in_time = StoragePointInTime::MinedPast(block_number);
        let mut futures = vec![];
        for (address, index) in changed_slots {
            futures.push(async move {
            let stratus_slot_value = loop {
                match self.stratus_chain.fetch_storage_at(&address, &index, point_in_time).await {
                    Ok(value) => break value,
                    Err(e) => tracing::warn!(reason = ?e, %address, %index, "failed to fetch slot value from stratus, retrying..."),
                }
            };

            let substrate_slot_value = loop {
                match self.substrate_chain.fetch_storage_at(&address, &index, StoragePointInTime::Mined).await {
                    Ok(value) => break value,
                    Err(e) => tracing::warn!(reason = ?e, %address, %index, "failed to fetch slot value from substrate, retrying..."),
                }
            };

            if stratus_slot_value != substrate_slot_value {
                tracing::error!(%address, %index, %point_in_time, "evm state mismatch between stratus and substrate");
                while let Err(e) = sqlx::query!(
                    "INSERT INTO slot_mismatches (address, index, block_number, stratus_value, substrate_value) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
                    address as _,
                    index as _,
                    block_number as _,
                    stratus_slot_value as _,
                    substrate_slot_value as _
                )
                .execute(&self.pool)
                .await {
                    tracing::warn!(?e, "failed to insert slot mismatch, retrying...");
                }
            }});
        }

        let mut buffer = futures::stream::iter(futures).buffer_unordered(100);
        while buffer.next().await.is_some() {}

        #[cfg(feature = "metrics")]
        inc_compare_final_state(start.elapsed());
    }

    pub async fn insert_transaction_mapping(&self, stratus_hash: Hash, new_transaction: &TransactionInput) {
        let new_hash = new_transaction.hash;
        let transaction_json = to_json_value(new_transaction);
        while let Err(e) = sqlx::query!(
            "INSERT INTO tx_hash_map (stratus_hash, substrate_hash, resigned_transaction) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            stratus_hash as _,
            new_hash as _,
            transaction_json as _,
        )
        .execute(&self.pool)
        .await
        {
            tracing::warn!(?e, "failed to insert transaction, retrying...");
        }
    }

    pub async fn get_mapped_transaction(&self, stratus_hash: Hash) -> anyhow::Result<Option<TransactionInput>> {
        sqlx::query!(
            r#"
            SELECT resigned_transaction
            FROM tx_hash_map
            WHERE stratus_hash=$1"#,
            stratus_hash as _
        )
        .fetch_optional(&self.pool)
        .await?
        .map(|row| row.resigned_transaction.try_into())
        .transpose()
    }

    pub async fn fetch_blocks(&self, blocks_to_fetch: u64) -> anyhow::Result<Vec<Block>> {
        let block_rows = sqlx::query!(
            r#"
            WITH cte AS (
                SELECT number
                FROM relayer_blocks
                WHERE finished = false
                ORDER BY number ASC
                LIMIT $1
            )
            UPDATE relayer_blocks r
                SET started = true
                FROM cte
                WHERE r.number = cte.number
                RETURNING r.number, r.payload"#,
            blocks_to_fetch as i64
        )
        .fetch_all(&self.pool)
        .await?;

        block_rows
            .into_iter()
            .sorted_by_key(|row| row.number)
            .map(|row| row.payload.try_into())
            .collect::<Result<_, _>>()
    }

    async fn check_nonces(&mut self, addresses: HashSet<Address>) -> anyhow::Result<()> {
        for from in addresses.into_iter() {
            if !self.out_of_sync_wallets.contains(&from) {
                let substrate_nonce = self.substrate_chain.fetch_transaction_count(&from).await?;
                let stratus_nonce = self.stratus_chain.fetch_transaction_count(&from).await?;
                if substrate_nonce != stratus_nonce {
                    self.insert_out_of_sync_wallet(from).await;
                }
            }
        }
        Ok(())
    }

    /// Polls the next block to be relayed and relays it to Substrate.
    #[tracing::instrument(name = "external_relayer::relay_blocks", skip_all, fields(block_number))]
    pub async fn relay_blocks(&mut self) -> anyhow::Result<Vec<BlockNumber>> {
        let blocks = self.fetch_blocks(self.blocks_to_fetch).await?;

        if blocks.is_empty() {
            tracing::info!("no blocks to relay");
            return Ok(vec![]);
        }

        let block_numbers: HashSet<BlockNumber> = blocks.iter().map(|block| block.number()).collect();
        let max_number = block_numbers.iter().max().copied().unwrap();

        // fill span
        Span::with(|s| s.rec_str("block_number", &max_number));

        let missing_blocks = self.missing_blocks(blocks.iter().map(|block| block.hash()).collect()).await?;
        if !missing_blocks.is_empty() {
            tracing::error!(?missing_blocks, "some blocks in this batch have not been mined in stratus");
            return Err(anyhow!("some blocks in this batch have not been mined in stratus"));
        }

        self.signer.sync_nonce(&self.substrate_chain).await?;
        let combined_transactions = self.combine_transactions(blocks).await?;
        let modified_slots = TransactionDag::get_slot_writes(&combined_transactions);
        let senders = combined_transactions
            .iter()
            .map(|tx| tx.input.signer)
            .filter(|address| address != &self.signer.address)
            .collect();

        let dag = TransactionDag::new(combined_transactions);

        let Ok(mismatched_blocks) = self.relay_dag(dag).await else {
            self.check_nonces(senders).await?;
            return Err(anyhow!("relay timedout, updated out of sync wallets and will try again"));
        };

        let ok_blocks: Vec<BlockNumber> = block_numbers.difference(&mismatched_blocks).copied().collect();

        if !mismatched_blocks.is_empty() {
            tracing::warn!(?mismatched_blocks, "some transactions mismatched");

            sqlx::query!(
                r#"UPDATE relayer_blocks
                SET finished = true, mismatched = true
                WHERE number = ANY($1)"#,
                &mismatched_blocks.iter().copied().collect_vec()[..] as _
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
        Ok(ok_blocks.into_iter().chain(mismatched_blocks.into_iter()).collect())
    }

    /// Compares the given receipt to the receipt returned by the pending transaction, retries until a receipt is returned
    /// to ensure the nonce was incremented. In case of a mismatch it returns an error describing what mismatched.
    #[tracing::instrument(name = "external_relayer::compare_receipt", skip_all, fields(tx_hash))]
    async fn compare_receipt(&self, mut stratus_tx: TransactionMined, substrate_pending_transaction: PendingTransaction<'_>) -> anyhow::Result<(), RelayError> {
        #[cfg(feature = "metrics")]
        let start_metric = metrics::now();

        let tx_hash: Hash = stratus_tx.input.hash;
        let block_number: BlockNumber = stratus_tx.block_number;

        tracing::info!(%block_number, %tx_hash, ?substrate_pending_transaction.tx_hash, "comparing receipts");

        // fill span
        Span::with(|s| s.rec_str("tx_hash", &tx_hash));

        let mut substrate_receipt = substrate_pending_transaction;
        let result = {
            let receipt = loop {
                match substrate_receipt.await {
                    Ok(r) => break r,
                    Err(e) => {
                        substrate_receipt = PendingTransaction::new(tx_hash, &self.substrate_chain);
                        tracing::warn!(reason = ?e);
                        continue;
                    }
                }
            };
            if let Some(substrate_receipt) = receipt {
                let _ = stratus_tx.execution.apply_receipt(&substrate_receipt);
                stratus_tx.execution.fix_logs_relayer_signer(&substrate_receipt);
                if let Err(compare_error) = stratus_tx.execution.compare_with_receipt(&substrate_receipt) {
                    let err_string = compare_error.to_string();
                    let error = log_and_err!("transaction mismatch!").context(err_string.clone());
                    self.save_mismatch(stratus_tx, substrate_receipt, &err_string).await;
                    error.map_err(|err| RelayError::Mismatch(block_number, err))
                } else {
                    Ok(())
                }
            } else {
                Err(RelayError::TransactionNotFound)
            }
        };

        #[cfg(feature = "metrics")]
        metrics::inc_compare_receipts(start_metric.elapsed());

        result
    }

    /// Save a transaction mismatch to postgres, if it fails, save it to a file.
    #[tracing::instrument(name = "external_relayer::save_mismatch", skip_all)]
    async fn save_mismatch(&self, stratus_receipt: TransactionMined, substrate_receipt: ExternalReceipt, err_string: &str) {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let tx_hash = stratus_receipt.input.hash;
        let block_number = stratus_receipt.block_number;

        tracing::info!(%block_number, %tx_hash, "saving transaction mismatch");

        let stratus_json = to_json_value(stratus_receipt);
        let substrate_json = to_json_value(substrate_receipt);
        let res = loop {
            match sqlx::query!(
                "INSERT INTO mismatches (hash, block_number, stratus_receipt, substrate_receipt, error) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
                &tx_hash as _,
                block_number as _,
                &stratus_json,
                &substrate_json,
                err_string
            )
            .execute(&self.pool)
            .await
            {
                Ok(res) => break res,
                Err(e) => tracing::error!(reason = ?e, %block_number, %tx_hash, "failed to insert row in pgsql, retrying"),
            }
        };

        if res.rows_affected() == 0 {
            tracing::info!(
                %block_number,
                %tx_hash,
                "transaction mismatch already in database (this should only happen if this block is being retried)."
            );
        };

        #[cfg(feature = "metrics")]
        metrics::inc_save_mismatch(start.elapsed());
    }

    pub async fn send_transaction(&self, tx_mined: TransactionMined, rlp: Bytes) -> PendingTransaction {
        let tx_hash = tx_mined.input.hash;
        loop {
            match self.substrate_chain.send_raw_transaction(rlp.clone()).await {
                Ok(tx) => break tx,
                Err(e) => {
                    tracing::warn!(
                        tx_nonnce = %tx_mined.input.nonce,
                        %tx_hash,
                        "substrate_chain.send_raw_transaction returned an error, checking if transaction was sent anyway"
                    );

                    if self.substrate_chain.fetch_transaction(tx_hash).await.unwrap_or(None).is_some() {
                        tracing::info!(%tx_hash, "transaction found on substrate");
                        return PendingTransaction::new(tx_hash, &self.substrate_chain);
                    }

                    tracing::warn!(reason = ?e, %tx_hash, "failed to send raw transaction, retrying...");
                    continue;
                }
            }
        }
    }

    /// Relays a transaction to Substrate and waits until the transaction is in the mempool by
    /// calling eth_getTransactionByHash. (infallible)
    #[tracing::instrument(name = "external_relayer::relay_and_check_mempool", skip_all, fields(tx_hash))]
    async fn relay_transaction(&self, tx_mined: TransactionMined) -> anyhow::Result<(), RelayError> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let tx_hash = tx_mined.input.hash;

        tracing::info!(?tx_mined.input.nonce, %tx_hash, "relaying transaction");

        // fill span
        Span::with(|s| s.rec_str("tx_hash", &tx_hash));

        let rlp = Transaction::from(tx_mined.input.clone()).rlp();
        let mut tx = self.send_transaction(tx_mined.clone(), rlp.clone()).await;

        #[cfg(feature = "metrics")]
        metrics::inc_relay_transaction(start.elapsed());
        loop {
            if let Err(error) = self.compare_receipt(tx_mined.clone(), tx).await {
                match error {
                    RelayError::TransactionNotFound => tracing::warn!(%tx_hash, "transaction not found in substrate, trying to resend"),
                    err => break Err(err),
                }
                tx = self.send_transaction(tx_mined.clone(), rlp.clone()).await;
            } else {
                break Ok(());
            }
        }
    }

    /// Relays a dag by removing its roots and sending them consecutively.
    async fn relay_component(&self, mut dag: TransactionDag) -> anyhow::Result<MismatchedBlocks, RelayError> {
        let start = Instant::now();

        let mut results = vec![];
        while let Some(roots) = dag.take_roots() {
            tracing::info!(elapsed=?start.elapsed().as_secs(), transaction_num=roots.len(), remaining=dag.txs_remaining(),"forwarding");
            let futures = roots
                .into_iter()
                .sorted()
                .map(|root_tx| tokio::time::timeout(Duration::from_secs(120), self.relay_transaction(root_tx)));
            let mut stream = futures::stream::iter(futures).buffered(30);
            while let Some(result) = stream.next().await {
                match result {
                    Ok(res) => results.push(res),
                    Err(_) => return Err(RelayError::RelayTimeout),
                }
            }
        }

        let errors = results.into_iter().filter_map(Result::err);

        let mut mismatched_blocks: MismatchedBlocks = HashSet::new();

        for error in errors {
            match error {
                RelayError::Mismatch(number, _) => mismatched_blocks.insert(number),
                _ => panic!("unexpected error"),
            };
        }

        Ok(mismatched_blocks)
    }

    // Split the dag and relay its components.
    #[tracing::instrument(name = "external_relayer::relay_dag", skip_all)]
    async fn relay_dag(&self, dag: TransactionDag) -> anyhow::Result<MismatchedBlocks, RelayError> {
        let start = Instant::now();

        tracing::debug!("relaying transactions");
        let mut results = vec![];
        let futures = dag.split_components().into_iter().map(|dag| self.relay_component(dag));
        let mut stream = futures::stream::iter(futures).buffered(60);
        while let Some(result) = stream.next().await {
            match result {
                Ok(res) => results.push(res),
                err => return err,
            }
        }

        let mismatched_blocks: MismatchedBlocks = results.into_iter().flatten().collect();

        #[cfg(feature = "metrics")]
        metrics::inc_relay_dag(start.elapsed());

        Ok(mismatched_blocks)
    }
}

pub struct ExternalRelayerClient {
    pool: Option<PgPool>,
    conn_lost: RwLock<bool>,
}

impl ExternalRelayerClient {
    /// Creates a new [`ExternalRelayerClient`].
    pub async fn new(config: ExternalRelayerClientConfig) -> Self {
        if !Path::new("blocks_json").exists() {
            create_dir("blocks_json").expect("creating a dir that does not exist should not fail");
        }
        tracing::info!(?config, "creating external relayer client");
        let storage = PgPoolOptions::new()
            .min_connections(config.connections)
            .max_connections(config.connections)
            .acquire_timeout(config.acquire_timeout)
            .connect(&config.url)
            .await;
        match storage {
            Ok(storage) => Self {
                pool: Some(storage),
                conn_lost: false.into(),
            },
            Err(err) => {
                tracing::error!(?err, "failed to establish connection to postgresql");
                Self {
                    pool: None,
                    conn_lost: true.into(),
                }
            }
        }
    }

    pub fn save_block_to_file(block_number: BlockNumber, block_json: &Value) -> anyhow::Result<()> {
        let file = File::create(format!("blocks_json/{}", block_number.as_u64()))?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, &block_json)?;
        writer.flush()?;
        Ok(())
    }

    /// Insert the block into the relayer_blocks table on pgsql to be processed by the relayer. Returns Err if
    /// the insertion fails.
    #[tracing::instrument(name = "external_relayer_client::send_to_relayer", skip_all, fields(block_number))]
    pub async fn send_to_relayer(&self, mut block: Block) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let block_number = block.header.number;
        tracing::info!(%block_number, "sending block to relayer");

        // strip bytecode
        for tx in block.transactions.iter_mut() {
            for (_, change) in tx.execution.changes.iter_mut() {
                change.bytecode = ExecutionValueChange::default();
            }
        }

        let block_json = to_json_value(block);
        // fill span
        Span::with(|s| s.rec_str("block_number", &block_number));
        let mut remaining_tries = 3;

        match *self.conn_lost.read().await {
            false =>
                if let Some(pool) = &self.pool {
                    while remaining_tries > 0 {
                        if let Err(e) = sqlx::query!(
                            r#"
                            INSERT INTO relayer_blocks
                            (number, payload)
                            VALUES ($1, $2)
                            ON CONFLICT (number) DO UPDATE
                            SET payload = EXCLUDED.payload"#,
                            block_number as _,
                            &block_json
                        )
                        .execute(pool)
                        .await
                        {
                            remaining_tries -= 1;
                            tracing::warn!(reason = ?e, ?remaining_tries, "failed to insert into relayer_blocks");
                        } else {
                            break;
                        }
                    }
                },
            true => Self::save_block_to_file(block_number, &block_json)?,
        }

        #[cfg(feature = "metrics")]
        metrics::inc_send_to_relayer(start.elapsed());

        match remaining_tries {
            0 => {
                let mut conn_lost = self.conn_lost.write().await;
                Self::save_block_to_file(block_number, &block_json)?;
                *conn_lost = true;
                Err(anyhow!(
                    "failed to insert block into relayer_blocks after 5 tries, switching to storing blocks in files."
                ))
            }
            _ => Ok(()),
        }
    }
}
