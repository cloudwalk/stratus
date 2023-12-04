use std::sync::Arc;

use ledger::evm::revm::Revm;
use ledger::storage::inmemory::InMemoryStorage;
use tracing_subscriber::EnvFilter;

pub fn init_testenv() -> (Revm, Arc<InMemoryStorage>) {
    // init tracing
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "debug");
    }
    let _ = tracing_subscriber::fmt().compact().with_env_filter(EnvFilter::from_default_env()).try_init();

    // init  evm
    let storage = Arc::new(InMemoryStorage::new());
    let evm = Revm::new(storage.clone());

    (evm, storage)
}

#[macro_export]
macro_rules! data {
    ($contract:ident.$function:tt( $($param:expr),* )) => {
        #[allow(clippy::vec_init_then_push)]
        #[allow(unused_mut)]
        {
            let mut tokens: Vec<ethabi::Token> = Vec::new();
            $(
                tokens.push($param.into());
            )*
            $contract.function(stringify!($function)).unwrap().encode_input(&tokens).unwrap().as_slice()
        }
    };
}
