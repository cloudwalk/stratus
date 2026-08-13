use alloy_rpc_types_trace::geth::GethDebugTracingOptions;

use crate::eth::types::Hash;

pub struct InspectorInput {
    pub tx_hash: Hash,
    pub opts: GethDebugTracingOptions,
    pub trace_unsuccessful_only: bool,
}
