use std::borrow::Cow;

use display_json::DebugAsJson;
use fake::Faker;

use crate::eth::codegen::error_sig_opt;
use crate::eth::primitives::Bytes;

/// Indicates how a transaction execution was finished.
#[derive(DebugAsJson, strum::Display, Clone, PartialEq, Eq, fake::Dummy, derive_new::new, serde::Serialize, serde::Deserialize, strum::EnumString)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionResult {
    /// Finished normally (RETURN opcode).
    #[strum(to_string = "success")]
    Success,

    /// Transaction execution finished with a reversion (REVERT opcode).
    #[strum(to_string = "reverted")]
    Reverted { reason: RevertReason },

    /// Transaction execution did not finish.
    #[strum(to_string = "halted")]
    Halted { reason: String },
}

impl ExecutionResult {
    pub fn is_success(&self) -> bool {
        matches!(self, ExecutionResult::Success)
    }
}

#[derive(Debug, thiserror::Error, serde::Serialize, serde::Deserialize, Default, Clone, PartialEq, Eq)]
pub struct RevertReason(pub Cow<'static, str>);

impl fake::Dummy<Faker> for RevertReason {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, _rng: &mut R) -> Self {
        RevertReason(Cow::Borrowed("reverted"))
    }
}

impl From<&'static str> for RevertReason {
    fn from(value: &'static str) -> Self {
        RevertReason(Cow::Borrowed(value))
    }
}

impl From<&Bytes> for RevertReason {
    fn from(value: &Bytes) -> Self {
        let sig: Cow<str> = error_sig_opt(value).unwrap_or_else(|| Cow::Owned(value.to_string()));
        Self(sig)
    }
}

impl std::fmt::Display for RevertReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::hex;

    use super::*;

    #[test]
    fn test_revert_reason_from_bytes() {
        // Test string revert
        let input = hex::decode(
            "08c379a00000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000\
0000000000014437573746f6d206572726f72206d657373616765000000000000000000000000",
        )
        .unwrap();
        let bytes = Bytes::from(input);
        let revert = RevertReason::from(&bytes);
        assert_eq!(revert.0, "Custom error message");

        // Test known error
        let input = hex::decode("22aa4404").unwrap();
        let bytes = Bytes::from(input);
        let revert = RevertReason::from(&bytes);
        assert_eq!(revert.0, "KnownError()");

        // Test unknown error
        let input = hex::decode("c39a0557").unwrap();
        let bytes = Bytes::from(input);
        let revert = RevertReason::from(&bytes);
        assert_eq!(revert.0, "0xc39a0557");
    }
}
