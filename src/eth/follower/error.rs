use stratus_macros::ErrorCode;

use crate::eth::types::ErrorCode;

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 4000]
pub enum ImporterError {
    #[error("importer is already running.")]
    #[error_code = 1]
    AlreadyRunning,

    #[error("importer is already shutdown.")]
    #[error_code = 2]
    AlreadyShutdown,

    #[error("failed to parse importer configuration.")]
    #[error_code = 3]
    ConfigParseError,

    #[error("failed to initialize importer.")]
    #[error_code = 4]
    InitError,
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 5000]
pub enum ConsensusError {
    #[error("consensus is temporarily unavailable for follower node.")]
    #[error_code = 1]
    Unavailable,

    #[error("consensus is set.")]
    #[error_code = 2]
    Set,

    #[error("failed to update consensus: Consensus is not set.")]
    #[error_code = 3]
    NotSet,
}
