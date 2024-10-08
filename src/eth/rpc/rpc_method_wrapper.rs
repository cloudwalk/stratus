use std::sync::Arc;

use jsonrpsee::types::Params;
use jsonrpsee::Extensions;

use super::rpc_parser::RpcExtensionsExt;
use super::RpcContext;
use crate::eth::primitives::StratusError;
use crate::infra::metrics::inc_executor_transaction_error_types;

pub fn call_error_metrics_wrapper<F, T>(function: F) -> impl Fn(Params<'_>, Arc<RpcContext>, Extensions) -> Result<T, StratusError> + Clone
where
    F: Fn(Params<'_>, Arc<RpcContext>, &Extensions) -> Result<T, StratusError> + Clone,
{
    move |params, ctx, extensions| {
        function(params, ctx, &extensions).inspect_err(|e| {
            let error_type = <&'static str>::from(e);
            let client = extensions.rpc_client();
            inc_executor_transaction_error_types(error_type, client);
        })
    }
}
