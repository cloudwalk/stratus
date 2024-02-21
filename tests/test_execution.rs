use std::sync::Arc;

use stratus::eth::evm::revm::Revm;

#[cfg(test)]
mod tests {
    use ethers_core::rand::thread_rng;
    // Adjust this import to include your Revm and related structs
    use fake::{Dummy, Faker};
    use nonempty::nonempty;
    use stratus::config::StorageConfig;
    use stratus::eth::primitives::test_accounts;
    use stratus::eth::primitives::TransactionInput;
    use stratus::eth::primitives::Wei;
    use stratus::eth::EthExecutor;

    use super::*;

    #[tokio::test]
    async fn test_execution() {
        let storage: StorageConfig = "inmemory".parse().unwrap(); //XXX we need to use a real storage
        let storage = storage.init().await.unwrap();
        let mut rng = thread_rng();
        let mut fake_transaction_input = TransactionInput::dummy_with_rng(&Faker, &mut rng);
        fake_transaction_input.nonce = 0.into();
        fake_transaction_input.gas_price = 0.into();
        fake_transaction_input.gas_limit = 0.into();

        let accounts = test_accounts();
        storage.save_accounts(accounts.clone()).await.unwrap();

        let address = accounts.last().unwrap().address.clone();
        fake_transaction_input.from = address;
        fake_transaction_input.value = Wei::from(0u64);

        let revm = Box::new(Revm::new(Arc::clone(&storage)));
        let executor = EthExecutor::new(nonempty![revm], Arc::clone(&storage));

        executor.transact(fake_transaction_input).await.unwrap();
    }
}
