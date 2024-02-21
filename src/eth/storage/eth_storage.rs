use async_trait::async_trait;

use crate::eth::primitives::Account;

/// EVM storage operations.
#[async_trait]
pub trait EthStorage: Send + Sync {}
/// Retrieves test accounts.
pub fn test_accounts() -> Vec<Account> {
    use hex_literal::hex;

    use crate::eth::primitives::Wei;

    [
        hex!("f39fd6e51aad88f6f4ce6ab8827279cfffb92266"),
        hex!("70997970c51812dc3a010c7d01b50e0d17dc79c8"),
        hex!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc"),
        hex!("15d34aaf54267db7d7c367839aaf71a00a2c6a65"),
        hex!("9965507d1a55bcc2695c58ba16fb37d819b0a4dc"),
        hex!("976ea74026e726554db657fa54763abd0c3a0aa9"),
    ]
    .into_iter()
    .map(|address| Account {
        address: address.into(),
        balance: Wei::TEST_BALANCE,
        ..Account::default()
    })
    .collect()
}
