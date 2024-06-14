use std::ops::Deref;
use std::ops::DerefMut;

use ethereum_types::Bloom;

use crate::gen_newtype_from;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(transparent)]
pub struct LogsBloom(Bloom);

impl Deref for LogsBloom {
    type Target = Bloom;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LogsBloom {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = LogsBloom, other = [u8; 256], Bloom);

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<LogsBloom> for Bloom {
    fn from(value: LogsBloom) -> Self {
        value.0
    }
}
