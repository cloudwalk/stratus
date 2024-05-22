use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use daggy::Dag;
use ethers_core::types::Transaction;

use itertools::Itertools;

use petgraph::algo::tarjan_scc;
use petgraph::graph::NodeIndex;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use super::primitives::Address;
use super::primitives::Block;
use super::primitives::BlockNumber;
use super::primitives::Hash;
use super::primitives::Index;
use super::primitives::SlotIndex;
use super::primitives::SlotValue;
use super::primitives::TransactionMined;
use crate::config::ExternalRelayerClientConfig;
use crate::config::ExternalRelayerServerConfig;
use crate::eth::evm::EvmExecutionResult;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::StratusStorage;
use crate::infra::BlockchainClient;
use crate::log_and_err;

type SlotConflictMap<'a> = HashMap<(Address, SlotIndex), (NodeIndex, RefCell<HashSet<Index>>)>;
type BalanceConflictMap<'a> = HashMap<Address, (NodeIndex, RefCell<HashSet<Index>>)>;
pub struct TransactionRelayer {
    storage: Arc<StratusStorage>,

    /// RPC client that will submit transactions.
    chain: BlockchainClient,
}

impl TransactionRelayer {
    /// Creates a new [`TransactionRelayer`].
    pub fn new(storage: Arc<StratusStorage>, chain: BlockchainClient) -> Self {
        tracing::info!(?chain, "creating transaction relayer");
        Self { storage, chain }
    }

    /// Forwards the transaction to the external blockchain if the execution was successful on our side.
    #[tracing::instrument(skip_all)]
    pub async fn forward(&self, tx_input: TransactionInput, evm_result: EvmExecutionResult) -> anyhow::Result<()> {
        tracing::debug!(hash = %tx_input.hash, "forwarding transaction");

        // handle local failure
        if evm_result.is_failure() {
            tracing::debug!("transaction failed in local execution");
            let tx_execution = TransactionExecution::new_local(tx_input, evm_result);
            self.storage.save_execution(tx_execution).await?;
            return Ok(());
        }

        // handle local success
        let pending_tx = self.chain.send_raw_transaction(Transaction::from(tx_input.clone()).rlp()).await?;

        let Some(receipt) = pending_tx.await? else {
            return Err(anyhow!("transaction did not produce a receipt"));
        };

        let status = match receipt.status {
            Some(status) => status.as_u32(),
            None => return Err(anyhow!("receipt did not report the transaction status")),
        };

        if status == 0 {
            tracing::warn!(?receipt.transaction_hash, "transaction result mismatch between stratus and external rpc. saving to json");
            let mut file = File::create(format!("data/mismatched_transactions/{}.json", receipt.transaction_hash)).await?;
            let json = serde_json::json!(
                {
                    "transaction_input": tx_input,
                    "stratus_execution": evm_result,
                    "substrate_receipt": receipt
                }
            );
            file.write_all(json.to_string().as_bytes()).await?;
            return Err(anyhow!("transaction succeeded in stratus but failed in substrate"));
        }

        Ok(())
    }
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
            substrate_chain: BlockchainClient::new_http(&config.forward_to).await?,
            pool,
        })
    }

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

        let (dag, component_roots) = Self::compute_tx_dags(block.transactions);

        self.relay_dag(dag, component_roots);

        Ok(Some(block_number))
    }


    fn compute_tx_dags(block_transactions: Vec<TransactionMined>) -> (Dag<Option<TransactionMined>, i32>, Vec<NodeIndex>) {
        let mut slot_conflicts: HashMap<&TransactionMined, HashSet<(&Address, &SlotIndex)>> = HashMap::new();
        let mut balance_conflicts: HashMap<&TransactionMined, HashSet<&Address>> = HashMap::new();
        let mut node_indexes: HashMap<&TransactionMined, NodeIndex> = HashMap::new();
        let mut dag = Dag::new();

        for tx in block_transactions.into_iter().sorted_by_key(|tx| tx.transaction_index) {
            let node_idx = dag.add_node(Some(tx));
            let tx = dag.node_weight(node_idx).unwrap().as_ref().unwrap();
            node_indexes.insert(tx, node_idx);
            for (address, change) in &tx.execution.changes {
                for (idx, slot_change) in &change.slots {
                    if slot_change.is_modified() {
                        slot_conflicts.entry(tx).or_default().insert((address, idx));
                    }
                }

                if change.balance.is_modified() {
                    balance_conflicts.entry(tx).or_default().insert(address);
                }
            }
        }

        for (i, (tx1, set1)) in slot_conflicts.iter().sorted_by_key(|(tx, _)| tx.transaction_index).enumerate() {
            for (tx2, set2) in slot_conflicts.iter().sorted_by_key(|(tx, _)| tx.transaction_index).skip(i) {
                if !set1.is_disjoint(set2) {
                    dag.add_edge(*node_indexes.get(tx1).unwrap(), *node_indexes.get(tx2).unwrap(), 1);
                }
            }
        }

        for (i, (tx1, set1)) in balance_conflicts.iter().sorted_by_key(|(tx, _)| tx.transaction_index).enumerate() {
            for (tx2, set2) in balance_conflicts.iter().sorted_by_key(|(tx, _)| tx.transaction_index).skip(i) {
                if !set1.is_disjoint(set2) {
                    dag.add_edge(*node_indexes.get(tx1).unwrap(), *node_indexes.get(tx2).unwrap(), 1);
                }
            }
        }

        (
            dag,
            tarjan_scc(&dag).into_iter().map(|component| component.into_iter().min().unwrap()).collect(),
        )
    }

    /// Forwards the transaction to the external blockchain if the execution was successful on our side.
    #[tracing::instrument(skip_all)]
    pub async fn relay(&self, tx_mined: TransactionMined) -> anyhow::Result<()> {
        tracing::debug!(hash = %tx_mined.input.hash, "forwarding transaction");
        if tx_mined.is_success() {
            let tx_input = tx_mined.input.clone();
            // handle success
            let Some(external_receipt) = self.substrate_chain.send_raw_transaction(Transaction::from(tx_input).rlp()).await?.await? else {
                return Err(anyhow!("transaction did not produce a receipt"));
            };

            let stratus_receipt = ExternalReceipt(tx_mined.into());

            if let Err(compare_error) = external_receipt.compare(&stratus_receipt) {
                let err_string = compare_error.to_string();
                let error = log_and_err!("transaction mismatch!").context(err_string.clone());
                sqlx::query!(
                    "INSERT INTO mismatches (stratus_receipt, substrate_receipt, error) VALUES ($1, $2, $3)",
                    serde_json::to_value(stratus_receipt)?,
                    serde_json::to_value(external_receipt)?,
                    err_string
                )
                .execute(&self.pool)
                .await?;
                return error;
            }
        } else {
            todo!("create empty transaction and send it or send it with 1 gas (we need to sign it... how??)")
        }
        Ok(())
    }

    pub async fn relay_dag(&self, dag: Dag<Option<TransactionMined>, i32>, component_roots: Vec<NodeIndex>) -> anyhow::Result<()> {
        todo!();
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
        let block_number = block.header.number;
        sqlx::query!("INSERT INTO relayer_blocks VALUES ($1, $2)", block_number as _, serde_json::to_value(block)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
