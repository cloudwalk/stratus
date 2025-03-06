use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

use alloy_primitives::hex;
use alloy_primitives::U256 as AlloyU256;
use anyhow::anyhow;
use anyhow::Result;
use ethereum_types::U256 as EthereumU256;
use revm::primitives::Bytecode;
use serde::Deserialize;
use serde::Serialize;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::Wei;

/// Represents the configuration of an Ethereum genesis.json file
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConfig {
    pub config: ChainConfig,
    pub nonce: String,
    pub timestamp: String,
    pub extraData: String,
    pub gasLimit: String,
    pub difficulty: String,
    pub mixHash: String,
    pub coinbase: String,
    pub alloc: BTreeMap<String, GenesisAccount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gasUsed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parentHash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseFeePerGas: Option<String>,
}

/// Chain configuration in genesis.json
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    pub chainId: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homesteadBlock: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eip150Block: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eip155Block: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eip158Block: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byzantiumBlock: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constantinopleBlock: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub petersburgBlock: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub istanbulBlock: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub berlinBlock: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub londonBlock: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shanghaiTime: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancunTime: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pragueTime: Option<u64>,
}

/// Account in the genesis.json file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAccount {
    pub balance: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<BTreeMap<String, String>>,
}

impl GenesisConfig {
    /// Loads a genesis.json file from the specified path
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let config: GenesisConfig = serde_json::from_reader(reader)?;
        Ok(config)
    }

    /// Searches for a genesis.json file in the current directory or a specified path
    pub fn find_genesis_file(custom_path: Option<&str>) -> Option<PathBuf> {
        if let Some(path) = custom_path {
            let path_buf = PathBuf::from(path);
            if path_buf.exists() {
                return Some(path_buf);
            }
        }

        // Searches in the current directory
        let default_path = PathBuf::from("genesis.json");
        if default_path.exists() {
            return Some(default_path);
        }

        None
    }

    /// Converts accounts from genesis.json to Stratus internal format
    pub fn to_stratus_accounts(&self) -> Result<Vec<Account>> {
        let mut accounts = Vec::new();
        let mut slots_to_add = Vec::new();

        for (addr_str, genesis_account) in &self.alloc {
            tracing::debug!("Processing account: {}", addr_str);

            let address = Address::from_str_hex(addr_str.trim_start_matches("0x"))?;

            // Convert balance from hex string to Wei
            let balance_str = genesis_account.balance.trim_start_matches("0x");
            let balance = if genesis_account.balance.starts_with("0x") {
                // For hex strings, use our custom implementation
                let bytes = hex::decode(balance_str)?;
                let mut array = [0u8; 32];
                let start = array.len() - bytes.len();
                array[start..].copy_from_slice(&bytes);
                let ethereum_u256 = EthereumU256::from_big_endian(&array);
                Wei::from(ethereum_u256)
            } else {
                // For decimal strings, use from_dec_str
                Wei::from(EthereumU256::from_dec_str(balance_str).map_err(|e| anyhow!("Failed to parse balance: {}", e))?)
            };
            tracing::debug!("Account balance: {}", balance.0);

            // Create the account
            let mut account = Account::new_empty(address);
            account.balance = balance;

            // Add code if it exists
            if let Some(code) = &genesis_account.code {
                let code_bytes = hex::decode(code.trim_start_matches("0x"))?;
                tracing::debug!("Added bytecode of length: {}", code_bytes.len());
                account.bytecode = Some(Bytecode::new_raw(code_bytes.into()));
            }

            // Add nonce if it exists
            if let Some(nonce) = &genesis_account.nonce {
                let nonce_str = nonce.trim_start_matches("0x");
                let nonce_value = if nonce_str.starts_with("0x") {
                    // For hex strings, use from_str_radix with base 16
                    let u256 = EthereumU256::from_str_radix(nonce_str.trim_start_matches("0x"), 16).map_err(|e| anyhow!("Failed to parse nonce: {}", e))?;
                    u256.as_u64()
                } else {
                    nonce_str.parse::<u64>()?
                };
                account.nonce = Nonce::from(nonce_value);
                tracing::debug!("Account nonce: {}", nonce_value);
            }

            // Add storage if it exists
            if let Some(storage) = &genesis_account.storage {
                tracing::debug!("Account has {} storage entries", storage.len());
                for (key_str, value_str) in storage {
                    // Convert key and value from hex string to U256
                    let key_str = key_str.trim_start_matches("0x");
                    let value_str = value_str.trim_start_matches("0x");

                    // Use AlloyU256 for initial parsing
                    let key_alloy = AlloyU256::from_str_radix(key_str, 16).map_err(|e| anyhow!("Failed to parse storage key: {}", e))?;
                    let value_alloy = AlloyU256::from_str_radix(value_str, 16).map_err(|e| anyhow!("Failed to parse storage value: {}", e))?;

                    // Convert from alloy_primitives::U256 to ethereum_types::U256
                    let key_bytes = key_alloy.to_be_bytes::<32>();
                    let key_ethereum = EthereumU256::from_big_endian(&key_bytes);

                    let value_bytes = value_alloy.to_be_bytes::<32>();
                    let value_ethereum = EthereumU256::from_big_endian(&value_bytes);

                    // Create a SlotIndex from the key
                    let slot_index = SlotIndex(key_ethereum);

                    // Create a Slot with the value
                    let slot_value = SlotValue(value_ethereum);
                    let slot = Slot::new(slot_index, slot_value);

                    // Add the slot to the account
                    tracing::debug!("Added storage slot: key={:?}, value={:?}", key_alloy, value_alloy);

                    // For now, just store the slots in a temporary vector
                    // TODO: Implement proper storage handling in a future PR
                    slots_to_add.push((address, slot));
                }
            }

            accounts.push(account);
        }

        Ok(accounts)
    }
}

// Implementation of hex string conversion to internal types
trait FromStrHex: Sized {
    fn from_str_hex(hex: &str) -> Result<Self>;
}

impl FromStrHex for Address {
    fn from_str_hex(hex: &str) -> Result<Self> {
        if hex.len() != 40 {
            return Err(anyhow!("Invalid address length"));
        }
        let bytes = hex::decode(hex)?;
        let mut array = [0u8; 20];
        array.copy_from_slice(&bytes);
        Ok(Address::new(array))
    }
}

impl FromStrHex for Wei {
    fn from_str_hex(hex: &str) -> Result<Self> {
        // Custom implementation for Wei
        let bytes = hex::decode(hex)?;
        let mut array = [0u8; 32];
        let start = array.len() - bytes.len();
        array[start..].copy_from_slice(&bytes);
        let ethereum_u256 = EthereumU256::from_big_endian(&array);
        Ok(Wei::from(ethereum_u256))
    }
}

impl FromStrHex for AlloyU256 {
    fn from_str_hex(hex: &str) -> Result<Self> {
        let bytes = hex::decode(hex)?;
        let mut array = [0u8; 32];
        let start = array.len() - bytes.len();
        array[start..].copy_from_slice(&bytes);
        Ok(AlloyU256::from_be_bytes(array))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_load_genesis_and_convert_accounts() {
        // Create a test GenesisConfig
        let json = r#"
        {
          "config": {
            "chainId": 2008
          },
          "nonce": "0x0000000000000042",
          "timestamp": "0x0",
          "extraData": "0x0000000000000000000000000000000000000000000000000000000000000000",
          "gasLimit": "0xffffffff",
          "difficulty": "0x1",
          "mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
          "coinbase": "0x0000000000000000000000000000000000000000",
          "alloc": {
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266": {
              "balance": "0xffffffffffffffffffffffffffffffff",
              "nonce": "0x1"
            },
            "0x1000000000000000000000000000000000000000": {
              "balance": "0x0",
              "code": "0x608060405234801561001057600080fd5b50600436106100365760003560e01c80632e64cec11461003b5780636057361d14610059575b600080fd5b610043610075565b60405161005091906100d9565b60405180910390f35b610073600480360381019061006e919061009d565b61007e565b005b60008054905090565b8060008190555050565b60008135905061009781610103565b92915050565b6000602082840312156100b3576100b26100fe565b5b60006100c184828501610088565b91505092915050565b6100d3816100f4565b82525050565b60006020820190506100ee60008301846100ca565b92915050565b6000819050919050565b600080fd5b61010c816100f4565b811461011757600080fd5b5056fea2646970667358221220404e37f487a89a932dca5e77faaf6ca2de59181e14ace3a8bd1f2b775b9f7a3764736f6c63430008090033",
              "storage": {
                "0x0000000000000000000000000000000000000000000000000000000000000000": "0x000000000000000000000000000000000000000000000000000000000000007b"
              }
            }
          }
        }"#;

        let genesis: GenesisConfig = serde_json::from_str(json).unwrap();
        let accounts = genesis.to_stratus_accounts().unwrap();

        // Check if we have the correct number of accounts
        assert_eq!(accounts.len(), 2);

        // Check the first account (EOA)
        let eoa = accounts
            .iter()
            .find(|a| a.address == Address::from_str("f39fd6e51aad88f6f4ce6ab8827279cfffb92266").unwrap())
            .unwrap();
        assert_eq!(eoa.nonce, Nonce::from(1u64));

        // Check the balance using custom from_str_hex
        let expected_balance = Wei::from_str_hex("ffffffffffffffffffffffffffffffff").unwrap();
        assert_eq!(eoa.balance, expected_balance);
        assert!(eoa.bytecode.is_none());

        // Check the second account (contract)
        let contract = accounts
            .iter()
            .find(|a| a.address == Address::from_str("1000000000000000000000000000000000000000").unwrap())
            .unwrap();
        assert_eq!(contract.balance, Wei::from(EthereumU256::from(0)));
        assert!(contract.bytecode.is_some());

        // Check the contract bytecode
        let bytecode = contract.bytecode.as_ref().unwrap();
        assert!(!bytecode.is_empty());

        // Check the contract storage (if implemented)
        // This will depend on how you implemented slot storage
    }
}
