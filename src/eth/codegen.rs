//! Auto-generated code.

use std::borrow::Cow;

use crate::eth::primitives::Address;
use crate::eth::primitives::LogFilter;
use crate::infra::metrics;

include!(concat!(env!("OUT_DIR"), "/contracts.rs"));
include!(concat!(env!("OUT_DIR"), "/signatures.rs"));

pub type SoliditySignature = &'static str;

pub type ContractName = &'static str;

/// Returns the contract name to be used in observability tasks.
pub fn contract_name(address: &Option<Address>) -> ContractName {
    let Some(address) = address else { return metrics::LABEL_MISSING };
    match CONTRACTS.get(address.as_slice()) {
        Some(contract_name) => contract_name,
        None => metrics::LABEL_UNKNOWN,
    }
}

/// Returns the function name to be used in observability tasks.
pub fn function_sig(bytes: impl AsRef<[u8]>) -> SoliditySignature {
    match function_sig_opt(bytes) {
        Some(signature) => signature,
        None => metrics::LABEL_UNKNOWN,
    }
}

/// Returns the function name to be used in observability tasks.
pub fn function_sig_opt(bytes: impl AsRef<[u8]>) -> Option<SoliditySignature> {
    let Some(id) = bytes.as_ref().get(..4) else {
        return Some(metrics::LABEL_MISSING);
    };
    SIGNATURES_4_BYTES.get(id).copied()
}

/// Returns the error name or string.
pub fn error_sig_opt(bytes: impl AsRef<[u8]>) -> Option<Cow<'static, str>> {
    bytes
        .as_ref()
        .get(..4)
        .and_then(|id| SIGNATURES_4_BYTES.get(id))
        .copied()
        .map(Cow::Borrowed)
        .or_else(|| {
            bytes.as_ref().get(4..).and_then(|bytes| {
                ethabi::decode(&[ethabi::ParamType::String], bytes)
                    .ok()
                    .and_then(|res| res.first().cloned())
                    .and_then(|token| token.into_string())
                    .map(Cow::Owned)
            })
        })
}

pub fn event_sig(bytes: impl AsRef<[u8]>) -> SoliditySignature {
    match event_sig_opt(bytes) {
        Some(signature) => signature,
        None => metrics::LABEL_UNKNOWN,
    }
}

pub fn event_sig_opt(bytes: impl AsRef<[u8]>) -> Option<SoliditySignature> {
    let bytes_slice = bytes.as_ref();
    if bytes_slice.len() != 32 {
        return Some(metrics::LABEL_MISSING);
    }
    let id: [u8; 32] = bytes_slice.try_into().ok()?;
    SIGNATURES_32_BYTES.get(&id).copied()
}

pub fn event_names_from_filter(filter: &LogFilter) -> SoliditySignature {
    let topics = &filter.original_input.topics;
    if topics.is_empty() {
        return metrics::LABEL_MISSING;
    }
    let topic0 = &topics[0];
    if topic0.is_empty() || topic0.contains(&None) {
        return metrics::LABEL_MISSING;
    }
    if let Some(Some(first_topic)) = topic0.0.first() {
        return event_sig(first_topic.as_ref());
    }
    
    metrics::LABEL_MISSING
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth::primitives::{LogFilterInput, LogFilterInputTopic, LogTopic};
    use crate::eth::storage::StratusStorage;
    use std::sync::Arc;

    #[test]
    fn test_event_sig_with_empty_bytes() {
        let result = event_sig(&[]);
        assert_eq!(result, metrics::LABEL_MISSING);
    }

    #[test]
    fn test_event_sig_with_wrong_size() {
        let result = event_sig(&[0u8; 4]); // 4 bytes instead of 32
        assert_eq!(result, metrics::LABEL_MISSING);
    }

    #[test]
    fn test_event_sig_with_unknown_signature() {
        let unknown_sig = [0xFFu8; 32];
        let result = event_sig(&unknown_sig);
        assert_eq!(result, metrics::LABEL_UNKNOWN);
    }

    #[test]
    fn test_event_names_from_filter_no_topics() {
        let storage = StratusStorage::new_test().unwrap();
        let filter_input = LogFilterInput::default();
        let filter = filter_input.parse(&Arc::new(storage)).unwrap();
        
        let result = event_names_from_filter(&filter);
        assert_eq!(result, metrics::LABEL_MISSING);
    }

    #[test]
    fn test_event_names_from_filter_with_none_topic() {
        let storage = StratusStorage::new_test().unwrap();
        let filter_input = LogFilterInput {
            topics: vec![LogFilterInputTopic(vec![None])],
            ..Default::default()
        };
        let filter = filter_input.parse(&Arc::new(storage)).unwrap();
        
        let result = event_names_from_filter(&filter);
        assert_eq!(result, metrics::LABEL_MISSING);
    }

    #[test]
    fn test_event_names_from_filter_with_unknown_topic() {
        let storage = StratusStorage::new_test().unwrap();
        let unknown_topic = LogTopic::from([0xFFu8; 32]);
        let filter_input = LogFilterInput {
            topics: vec![LogFilterInputTopic(vec![Some(unknown_topic)])],
            ..Default::default()
        };
        let filter = filter_input.parse(&Arc::new(storage)).unwrap();
        
        let result = event_names_from_filter(&filter);
        assert_eq!(result, metrics::LABEL_UNKNOWN);
    }
}
