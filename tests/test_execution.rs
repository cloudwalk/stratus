use ethers_core::rand::thread_rng;
// Adjust this import to include your Revm and related structs
use fake::{Dummy, Faker};
use stratus::config::CommonConfig;
use stratus::eth::primitives::test_accounts;
use stratus::eth::primitives::TransactionInput;
use stratus::eth::primitives::Wei;
use stratus::init_global_services;

#[tokio::test]
async fn test_execution() {
    let config = init_global_services::<CommonConfig>();

    let storage = config.init_stratus_storage().await.unwrap();
    let mut rng = thread_rng();
    let mut fake_transaction_input = TransactionInput::dummy_with_rng(&Faker, &mut rng);
    fake_transaction_input.chain_id = Some(2008u64.into());
    fake_transaction_input.nonce = 0u64.into();
    fake_transaction_input.gas_price = 0u64.into();
    fake_transaction_input.gas_limit = 0u64.into();

    let accounts = test_accounts();
    storage.save_accounts_to_perm(accounts.clone()).await.unwrap();

    let address = accounts.last().unwrap().address.clone();
    fake_transaction_input.from = address;
    fake_transaction_input.value = Wei::from(0u64);

    let executor = config.init_executor(storage);

    executor.transact(fake_transaction_input).await.unwrap();
}
