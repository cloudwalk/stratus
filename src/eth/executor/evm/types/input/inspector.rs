use alloy_rpc_types_trace::geth::GethDebugTracingOptions;

use crate::eth::types::CallInput;
use crate::eth::types::Hash;
use crate::eth::types::PointInTime;

pub enum InspectorInput {
    /// Traces an already-mined transaction, looked up by hash (`debug_traceTransaction`).
    Transaction {
        tx_hash: Hash,
        opts: GethDebugTracingOptions,
        trace_unsuccessful_only: bool,
    },

    /// Traces a synthetic call that was never signed or broadcast, against a chosen point in time (`debug_traceCall`).
    Call {
        call: CallInput,
        point_in_time: PointInTime,
        opts: GethDebugTracingOptions,
    },
}
