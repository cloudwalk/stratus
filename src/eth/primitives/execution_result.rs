/// Indicates how a transaction execution was finished.
#[derive(Debug, strum::Display, Clone, PartialEq, Eq, fake::Dummy, derive_new::new, serde::Serialize, serde::Deserialize, strum::EnumString)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionResult {
    /// Finished normally (RETURN opcode).
    #[strum(to_string = "success")]
    Success,

    /// Transaction execution finished with a reversion (REVERT opcode).
    #[strum(to_string = "reverted")]
    Reverted,

    /// Transaction execution did not finish.
    #[strum(to_string = "halted")]
    Halted { reason: String },
}
