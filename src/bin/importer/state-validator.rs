use std::sync::Arc;
use std::time::Duration;

use stratus::config::{StateValidatorConfig, ValidatorMethodConfig};
use stratus::eth::primitives::BlockNumber;
use stratus::eth::storage::StratusStorage;
use stratus::infra::BlockchainClient;
use stratus::init_global_services;

const RPC_TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::main()]
async fn main() -> anyhow::Result<()> {
    // init services
    let config: StateValidatorConfig = init_global_services();
    let storage = config.init_storage().await?;

    let interval = BlockNumber::from(config.interval);

    let mut latest_compared_block = BlockNumber::ZERO;
    loop {
        let current_block = storage.read_current_block_number().await?;
        if current_block - latest_compared_block >= interval {
            let result = validate_state(
                &config.method,
                Arc::clone(&storage),
                latest_compared_block,
                latest_compared_block + interval,
                config.sample_size,
                config.seed,
            )
            .await;
            if let Err(err) = result {
                panic!("{}", err);
            }
            latest_compared_block = latest_compared_block + interval;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn validate_state(
    method: &ValidatorMethodConfig,
    storage: Arc<StratusStorage>,
    start: BlockNumber,
    end: BlockNumber,
    max_sample_size: u64,
    seed: u64,
) -> anyhow::Result<()> {
    match method {
        ValidatorMethodConfig::Rpc { url } => {
            let chain = BlockchainClient::new(url, RPC_TIMEOUT)?;
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
    println!("Validating state {:?}, {:?}", start, end);
    let seed = match seed {
        0 => None,
        n => Some(n),
    };
    let slots = storage.get_slots_sample(start, end, max_sample_size, seed).await?;
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
