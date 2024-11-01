use std::sync::Arc;

use cfg_if::cfg_if;
use jsonrpsee::types::Params;
use jsonrpsee::Extensions;

use crate::eth::primitives::StratusError;
use crate::eth::rpc::reject_unknown_client;
use crate::eth::rpc::rpc_parser::RpcExtensionsExt;
use crate::eth::rpc::RpcContext;

cfg_if! {
    if #[cfg(feature = "metrics")] {
        use crate::infra::metrics::inc_rpc_error_response;

        fn metrify_stratus_error(err: &StratusError, extensions: &Extensions, method_name: &'static str) {
            let error_type = <&'static str>::from(err);
            let client = extensions.rpc_client();
            inc_rpc_error_response(error_type, client, method_name);
        }

        pub fn metrics_wrapper<F, T>(function: F, method_name: &'static str) -> impl Fn(Params<'_>, Arc<RpcContext>, Extensions) -> Result<T, StratusError> + Clone
        where
            F: Fn(Params<'_>, Arc<RpcContext>, &Extensions) -> Result<T, StratusError> + Clone,
        {
            move |params, ctx, extensions| {
                reject_unknown_client(extensions.rpc_client())?;
                function(params, ctx, &extensions).inspect_err(|e| metrify_stratus_error(e, &extensions, method_name))
            }
        }
    } else {
        pub fn metrics_wrapper<F, T>(function: F, _method_name: &'static str) -> impl Fn(Params<'_>, Arc<RpcContext>, Extensions) -> Result<T, StratusError> + Clone
        where
            F: Fn(Params<'_>, Arc<RpcContext>, &Extensions) -> Result<T, StratusError> + Clone,
        {
            move |params, ctx, extensions| {
                reject_unknown_client(extensions.rpc_client())?;
                function(params, ctx, &extensions)
            }
        }
    }
}
