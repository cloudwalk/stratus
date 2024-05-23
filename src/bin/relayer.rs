mod importer_online;

use stratus::config::ExternalRelayerConfig;
use stratus::GlobalServices;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<ExternalRelayerConfig>::init()?;
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: ExternalRelayerConfig) -> anyhow::Result<()> {
    // init services
    let backoff = config.relayer.backoff;
    let relayer = config.relayer.init().await?;

    loop {
        let block_number = match relayer.relay_next_block().await {
            Ok(bnum) => bnum,
            Err(err) => {
                tracing::error!(?err, "error relaying next block");
                continue;
            }
        };

        match block_number {
            Some(block_number) => tracing::info!(?block_number, "relayed"),
            None => tokio::time::sleep(backoff).await,
        };
    }
}
