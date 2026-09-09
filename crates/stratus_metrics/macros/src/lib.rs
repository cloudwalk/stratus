//! Function-like and attribute macros for the `stratus_metrics` crate.

// Panics in proc macros abort compilation with an error message, so they are safe to use here.
#![allow(clippy::panic)]

use proc_macro::TokenStream;

mod metrics;
mod timed_attribute;

/// Defines the metrics of a group and generates the functions to record them.
///
/// Each metric has a description, a kind, a name, and optional labels. The
/// first metric kind in the list below generates `inc_<name>` and
/// `inc_n_<name>`, the second and third only `inc_<name>`, and the gauge
/// generates `set_<name>`, `inc_<name>`, and `dec_<name>`:
///
/// ```ignore
/// metrics! {
///     group: storage_read,
///
///     "Time executing storage read_block operation."
///     histogram_duration storage_read_block{storage, success},
///
///     "Number of storage reads."
///     counter storage_reads{storage, hit},
/// }
/// ```
///
/// For each group, the macro also generates a `METRIC_<NAME>` constant for
/// every metric and a `metrics_for_<group>()` function returning the metric
/// definitions. Label values are passed as arguments to the generated
/// functions and must follow the order in the metric definition.
#[proc_macro]
pub fn metrics(input: TokenStream) -> TokenStream {
    match metrics::expand(input.into()) {
        Ok(expanded) => expanded.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Times a function and records its duration in a `stratus_metrics` histogram.
///
/// The first argument is the name of a `histogram_duration` metric. Label
/// values are positional and must follow the order in the metric definition.
///
/// ```ignore
/// #[timed(
///     storage_read_block,
///     labels(
///         storage = label::PERM,
///         success = result.is_ok(),
///     )
/// )]
/// fn read_block(...) -> Result<..., ...> {
///     // ...
/// }
/// ```
///
/// Bare function parameters are converted to owned `MetricLabelValue`s before
/// the function runs, without requiring the parameter to implement `Clone`:
///
/// ```ignore
/// #[timed(executor_inspect, labels(trace_type))]
/// fn inspect(trace_type: String) -> Result<(), StratusError> {
///     // ...
/// }
/// ```
///
/// Closures derive and convert labels from input parameters before the body can
/// consume them. Closure argument names must match function parameter names and
/// receive references to those parameters:
///
/// ```ignore
/// #[timed(
///     executor_external_transaction,
///     labels(
///         contract = |input| contract_name(&input.execution_info.to),
///         function = |input| function_sig(&input.execution_info.input),
///     )
/// )]
/// fn execute(input: TransactionInput) -> Result<(), StratusError> {
///     // ...
/// }
/// ```
///
/// Other expressions are converted after the function and may inspect its
/// return value through the `result` binding:
///
/// ```ignore
/// #[timed(
///     storage_save_execution,
///     labels(success = result.is_ok()),
/// )]
/// fn save_execution(...) -> Result<(), StorageError> {
///     // ...
/// }
/// ```
///
/// Both synchronous and asynchronous functions are supported. `const fn` and
/// `unsafe fn` are rejected.
#[proc_macro_attribute]
pub fn timed(args: TokenStream, input: TokenStream) -> TokenStream {
    match timed_attribute::expand(args.into(), input.into()) {
        Ok(expanded) => expanded.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
