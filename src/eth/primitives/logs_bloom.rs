use std::ops::Deref;
use std::ops::DerefMut;

use alloy_primitives::Bloom;
use alloy_primitives::BloomInput;

use crate::eth::primitives::Log;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(transparent)]
pub struct LogsBloom(pub Bloom);

impl LogsBloom {
    pub fn accrue_log(&mut self, log: &Log) {
        self.accrue(BloomInput::Raw(log.address.as_ref()));
        for topic in log.topics_non_empty() {
            self.accrue(BloomInput::Raw(topic.as_ref()));
        }
    }
}

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

impl From<[u8; 256]> for LogsBloom {
    fn from(value: [u8; 256]) -> Self {
        Self(Bloom::from(value))
    }
}

impl From<Bloom> for LogsBloom {
    fn from(value: Bloom) -> Self {
        let bytes: [u8; 256] = *value.0;
        Self(Bloom::from(bytes))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<LogsBloom> for Bloom {
    fn from(value: LogsBloom) -> Self {
        value.0
    }
}

impl From<LogsBloom> for [u8; 256] {
    fn from(value: LogsBloom) -> Self {
        value.0.into_array()
    }
}

#[cfg(test)]
mod tests {
    use hex_literal::hex;

    use super::*;

    #[test]
    fn compute_bloom() {
        let log1 = Log {
            address: hex!("c6d1efd908ef6b69da0749600f553923c465c812").into(),
            topic0: Some(hex!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef").into()),
            topic1: Some(hex!("000000000000000000000000d1ff9b395856e5a6810f626eca09d61d34fce3b8").into()),
            topic2: Some(hex!("000000000000000000000000081d2c5b26db6f6e0944e4725b3b61b26e25dd8a").into()),
            topic3: None,
            data: hex!("0000000000000000000000000000000000000000000000000000000005f5e100").as_ref().into(),
            index: None,
        };
        let log2 = Log {
            address: hex!("b1f571b3254c99a0a562124738f0193de2b2b2a9").into(),
            topic0: Some(hex!("8d995e7fbf7a5ef41cee9e6936368925d88e07af89306bb78a698551562e683c").into()),
            topic1: Some(hex!("000000000000000000000000081d2c5b26db6f6e0944e4725b3b61b26e25dd8a").into()),
            topic2: None,
            topic3: None,
            data: hex!(
                "0000000000000000000000000000000000000000000000000000000000004dca00000000\
                0000000000000000000000000000000000000000000000030c9281f0"
            )
            .as_ref()
            .into(),
            index: None,
        };
        let mut bloom = LogsBloom::default();
        bloom.accrue_log(&log1);
        bloom.accrue_log(&log2);

        let expected: LogsBloom = hex!(
            "000000000400000000000000000000000000000000000000000000000000\
        00000000000000000000000000000000000000080000000000000000000000000000000000000000000000000008\
        00008400202000000000002000000000000000000000000000000010000000000000000000000000040000000000\
        00100000000000000000000000000000000000000000000000000000000000000000000000001000000000000100\
        00000000000000000000000000000000000000000000080000000002000000000000000000000000000000002000\
        000000440000000000000000000000000000000000000000000000000000000000000000000000000000"
        )
        .into();

        assert_eq!(bloom, expected);
    }
}
