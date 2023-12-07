use std::sync::Arc;

use ledger::eth::evm::revm::Revm;
use ledger::eth::storage::InMemoryStorage;

pub fn init_testenv() -> (Revm, Arc<InMemoryStorage>) {
    // init tracing
    ledger::infra::init_tracing();

    // init  evm
    let storage = Arc::new(InMemoryStorage::new());
    let evm = Revm::new(storage.clone());

    (evm, storage)
}

#[macro_export]
macro_rules! data {
    ($contract:ident.$function:tt( $($param:expr),* )) => {
        {
            #[allow(unused_mut)]
            let tokens: Vec<ethabi::Token> = vec![
                $(
                    $param.into(),
                )*
            ];
            $contract.function(stringify!($function)).unwrap().encode_input(&tokens).unwrap()
        }
    };
}
