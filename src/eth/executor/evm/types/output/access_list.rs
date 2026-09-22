use derive_more::IntoIterator;
use display_json::DebugAsJson;
use revm_state::EvmState;

use crate::eth::executor::evm::RevmResultAndState;
use crate::eth::types::Address;
use crate::eth::types::SlotIndex;
use crate::eth::types::StratusError;

#[derive(serde::Serialize, serde::Deserialize, DebugAsJson, Clone, IntoIterator)]
pub struct AccessListOutput {
    #[into_iterator(owned, ref, ref_mut)]
    access_list: Vec<(Address, Vec<SlotIndex>)>,
}

impl AccessListOutput {
    fn parse_revm_state(revm_state: EvmState) -> Vec<(Address, Vec<SlotIndex>)> {
        revm_state
            .into_iter()
            .map(|(address, account)| {
                let slot_list = account.storage.into_keys().map(Into::into).collect();
                (address.into(), slot_list)
            })
            .collect()
    }
}

impl TryFrom<RevmResultAndState> for AccessListOutput {
    type Error = StratusError;

    fn try_from(value: RevmResultAndState) -> Result<Self, Self::Error> {
        let access_list = Self::parse_revm_state(value.state);
        tracing::debug!(?access_list, "evm executed");

        Ok(Self { access_list })
    }
}
