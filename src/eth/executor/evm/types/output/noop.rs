use display_json::DebugAsJson;

use crate::eth::executor::evm::RevmResultAndState;
use crate::eth::types::StratusError;

#[derive(serde::Serialize, DebugAsJson)]
pub struct NoopOutput;

impl TryFrom<RevmResultAndState> for NoopOutput {
    type Error = StratusError;

    fn try_from(_: RevmResultAndState) -> Result<Self, Self::Error> {
        Ok(Self)
    }
}
