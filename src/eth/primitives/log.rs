use ethers_core::types::Log as EthersLog;
use revm::primitives::Log as RevmLog;

use crate::eth::primitives::Address;

#[derive(Debug, Clone)]
pub struct Log {
    _address: Address,
    _inner: EthersLog,
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<RevmLog> for Log {
    fn from(value: RevmLog) -> Self {
        EthersLog {
            address: value.address.0 .0.into(),
            topics: value.topics.into_iter().map(|x| x.0.into()).collect(),
            data: value.data.0.into(),
            removed: Some(false),
            ..Default::default()
        }
        .into()
    }
}

impl From<EthersLog> for Log {
    fn from(value: EthersLog) -> Self {
        Self {
            _address: value.address.into(),
            _inner: value,
        }
    }
}
