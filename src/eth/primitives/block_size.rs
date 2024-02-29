use crate::infra::metrics::LabelValue;

/// Block size representation to be used in labels.
#[derive(Debug, Clone, Copy, strum::EnumString, strum::Display)]
pub enum BlockSize {
    #[strum(serialize = "empty")]
    Empty,
    #[strum(serialize = "small")]
    Small,
    #[strum(serialize = "medium")]
    Medium,
    #[strum(serialize = "large")]
    Large,
    #[strum(serialize = "huge")]
    Huge,
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<BlockSize> for LabelValue {
    fn from(value: BlockSize) -> Self {
        LabelValue::Some(value.to_string())
    }
}
