use std::sync::Arc;

use rand::Rng;
use stratus::config::StateValidatorConfig;
use stratus::config::ValidatorMethodConfig;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::storage::StratusStorage;
use stratus::infra::BlockchainClient;
use stratus::init_global_services;
use tokio::task::JoinSet;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // init services
    let config: StateValidatorConfig = init_global_services();
    let storage = config.init_storage().await?;

    let interval = BlockNumber::from(config.interval);

    let mut latest_compared_block = BlockNumber::ZERO;

    let mut futures = JoinSet::new();
    loop {
        let current_block = storage.read_current_block_number().await?;
        if current_block - latest_compared_block >= interval && futures.len() < config.concurrent_tasks as usize {
            let future = validate_state(
                config.method.clone(),
                Arc::clone(&storage),
                latest_compared_block,
                latest_compared_block + interval,
                config.sample_size,
                config.seed,
            );

            futures.spawn(future);

            latest_compared_block = latest_compared_block + interval;
        } else if let Some(res) = futures.join_next().await {
            res??;
        }
    }
}

async fn validate_state(
    method: ValidatorMethodConfig,
    storage: Arc<StratusStorage>,
    start: BlockNumber,
    end: BlockNumber,
    max_sample_size: u64,
    seed: u64,
) -> anyhow::Result<()> {
    match method {
        ValidatorMethodConfig::Rpc { url } => {
            let chain = BlockchainClient::new(&url).await?;
            validate_state_rpc(&chain, storage, start, end, max_sample_size, seed).await
        }
        _ => todo!(),
    }
}

async fn validate_state_rpc(
    chain: &BlockchainClient,
    storage: Arc<StratusStorage>,
    start: BlockNumber,
    end: BlockNumber,
    max_sample_size: u64,
    seed: u64,
) -> anyhow::Result<()> {
    tracing::debug!("Validating state {:?}, {:?}", start, end);
    let seed = match seed {
        0 => {
            let mut rng = rand::thread_rng();
            rng.gen()
        }
        n => n,
    };
    let slots = storage.read_slots_sample(start, end, max_sample_size, seed).await?;
    for sampled_slot in slots {
        let expected_value = chain
            .get_storage_at(
                &sampled_slot.address,
                &sampled_slot.index,
                stratus::eth::primitives::StoragePointInTime::Past(sampled_slot.block_number),
            )
            .await?;

        if sampled_slot.value != expected_value {
            return Err(anyhow::anyhow!(
                "State mismatch on slot {:?}, expected value: {:?}, found: {:?}",
                sampled_slot,
                expected_value,
                sampled_slot.value
            ));
        }
    }
    Ok(())
}
