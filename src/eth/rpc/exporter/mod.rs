//! Exporter: serves importer-facing (follower sync) requests on a dedicated runtime.
//!
//! The importer already runs on its own dedicated runtime on the follower. The exporter is the
//! leader-side counterpart: importer-facing RPC methods are dispatched onto a dedicated blocking
//! pool, so follower sync traffic cannot stall behind a saturated main blocking pool.

pub mod config;
pub mod runtime;

pub use config::ExporterConfig;
pub use runtime::ExporterRuntime;
pub use runtime::ExporterRuntimeConfig;
