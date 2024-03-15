use ethers_core::rand::thread_rng;
// Adjust this import to include your Revm and related structs
use fake::{Dummy, Faker};
use stratus::config::CommonConfig;
use stratus::config::MetricsHistogramKind;
use stratus::config::PermanentStorageConfig;
use stratus::config::PermanentStorageKind;
use stratus::config::TemporaryStorageConfig;
use stratus::config::TemporaryStorageKind;
use stratus::eth::primitives::test_accounts;
use stratus::eth::primitives::TransactionInput;
use stratus::eth::primitives::Wei;

#[tokio::test]
async fn test_execution() {
    let config = CommonConfig {
        perm: PermanentStorageConfig {
            perm_kind: PermanentStorageKind::InMemory,
            perm_connections: 1,
            perm_timeout_millis: 1000,
        },
        temp: TemporaryStorageConfig {
            temp_kind: TemporaryStorageKind::InMemory,
        },
        num_evms: 1usize,
        num_async_threads: 1usize,
        num_blocking_threads: 1usize,
        metrics_histogram_kind: MetricsHistogramKind::Summary,
        enable_genesis: false,
        nocapture: false,
    };
    let storage = config.init_storage().await.unwrap();
    let mut rng = thread_rng();
    let mut fake_transaction_input = TransactionInput::dummy_with_rng(&Faker, &mut rng);
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
