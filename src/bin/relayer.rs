mod importer_online;

use stratus::config::ExternalRelayerConfig;
use stratus::ext::traced_sleep;
use stratus::ext::SleepReason;
#[cfg(feature = "metrics")]
use stratus::infra::metrics;
use stratus::utils::DropTimer;
use stratus::GlobalServices;
use stratus::GlobalState;

const TASK_NAME: &str = "relayer";

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<ExternalRelayerConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: ExternalRelayerConfig) -> anyhow::Result<()> {
    let _timer = DropTimer::start("relayer");

    tracing::info!(?TASK_NAME, "starting");

    // init services
    let backoff = config.relayer.backoff;
    let mut relayer = config.relayer.init().await?;

    loop {
        if GlobalState::warn_if_shutdown(TASK_NAME) {
            return Ok(());
        };

        #[cfg(feature = "metrics")]
        let start = metrics::now();
        let blocks = match relayer.relay_blocks().await {
            Ok(block_number) => {
                #[cfg(feature = "metrics")]
                metrics::inc_relay_next_block(start.elapsed());
                block_number
            }
            Err(e) => {
                tracing::error!(reason = ?e, "error relaying next block");
                continue;
            }
        };

        match blocks.len() {
            0 => {
                tracing::info!("no blocks relayed");
                traced_sleep(backoff, SleepReason::RetryBackoff).await;
            }
            _ => tracing::info!(?blocks, "relayed"),
        }
    }
}
