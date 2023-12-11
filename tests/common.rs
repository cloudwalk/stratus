use std::sync::Arc;

use ledger::eth::evm::revm::Revm;
use ledger::eth::storage::inmemory::InMemoryStorage;
use ledger::eth::EthExecutor;

pub fn init_testenv() -> EthExecutor {
    // init tracing
    ledger::infra::init_tracing();

    // init  evm
    let storage = Arc::new(InMemoryStorage::default());
    let evm = Revm::new(storage.clone());
    EthExecutor::new(Box::new(evm), storage.clone(), storage)
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
            $contract.function(stringify!($function)).unwrap().encode_input(&tokens).unwrap().into()
        }
    };
}
