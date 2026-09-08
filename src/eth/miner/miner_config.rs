use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;
use stratus_macros::CliOverrides;

use crate::GlobalState;
use crate::NodeMode;
use crate::eth::miner::Miner;
use crate::eth::storage::StratusStorage;
use crate::ext::not;
use crate::ext::parse_duration;

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

#[derive(Parser, DebugAsJson, Clone, serde::Deserialize, serde::Serialize, CliOverrides)]
#[serde(default, deny_unknown_fields)]
pub struct MinerConfig {
    /// Target block time.
    #[arg(long = "block-mode", default_value = "automine")]
    pub block_mode: MinerMode,
}

impl Default for MinerConfig {
    fn default() -> Self {
        Self {
            block_mode: MinerMode::Automine,
        }
    }
}

impl MinerConfig {
    /// Inits [`Miner`] with the appropriate mining mode based on the node mode.
    pub async fn init(&self, storage: Arc<StratusStorage>) -> anyhow::Result<Arc<Miner>> {
        tracing::info!(config = ?self, "creating block miner");

        let mode = match GlobalState::get_node_mode() {
            NodeMode::Follower | NodeMode::FakeLeader => {
                if not(self.block_mode.is_external()) {
                    tracing::error!(block_mode = ?self.block_mode, "invalid block-mode, a follower's miner can only start as external!");
                }
                MinerMode::External
            }
            NodeMode::Leader => self.block_mode,
        };

        self.init_with_mode(mode, storage).await
    }

    /// Inits [`Miner`] with a specific mining mode, regardless of node mode.
    pub async fn init_with_mode(&self, mode: MinerMode, storage: Arc<StratusStorage>) -> anyhow::Result<Arc<Miner>> {
        tracing::info!(config = ?self, mode = ?mode, "creating block miner with specific mode");

        // create miner
        let miner = Miner::new(Arc::clone(&storage), mode);
        let miner = Arc::new(miner);

        if let MinerMode::Interval(block_time) = mode {
            miner.start_interval_mining(block_time).await;
        }

        Ok(miner)
    }
}

// -----------------------------------------------------------------------------
// Mode
// -----------------------------------------------------------------------------

/// Indicates when the miner will mine new blocks.
#[derive(Debug, Clone, Copy, PartialEq, strum::EnumIs)]
pub enum MinerMode {
    /// Mines a new block for each transaction execution.
    Automine,

    /// Mines a new block at specified interval.
    Interval(Duration),

    /// Does not automatically mines a new block. A call to `mine_*` must be executed to mine a new block.
    External,
}

impl FromStr for MinerMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            "automine" => Ok(Self::Automine),
            "external" => Ok(Self::External),
            s => {
                let block_time = parse_duration(s)?;
                Ok(Self::Interval(block_time))
            }
        }
    }
}

impl serde::Serialize for MinerMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Automine => serializer.serialize_str("automine"),
            Self::Interval(duration) => serializer.serialize_str(&humantime::format_duration(*duration).to_string()),
            Self::External => serializer.serialize_str("external"),
        }
    }
}

impl<'de> serde::Deserialize<'de> for MinerMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::from_str(&value).map_err(serde::de::Error::custom)
    }
}
