#[allow(clippy::module_inception)]
mod postgres_external_rpc;

pub use postgres_external_rpc::PostgresExternalRpcStorage;
pub use postgres_external_rpc::PostgresExternalRpcStorageConfig;
