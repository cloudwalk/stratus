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
use serde::Deserialize;
use serde::Serialize;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Nonce;
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

        // Check for environment-specific genesis file
        if let Ok(env) = std::env::var("ENV") {
            let env_path = format!("config/genesis.{}.json", env.to_lowercase());
            let env_path_buf = PathBuf::from(&env_path);
            if env_path_buf.exists() {
                return Some(env_path_buf);
            }
        }

        // Check for default local genesis file
        let local_path = PathBuf::from("config/genesis.local.json");
        if local_path.exists() {
            return Some(local_path);
        }

        // Fallback to current directory
        let default_path = PathBuf::from("genesis.json");
        if default_path.exists() {
            return Some(default_path);
        }

        None
    }

    /// Converts accounts from genesis.json to Stratus internal format
    pub fn to_stratus_accounts(&self) -> Result<Vec<Account>> {
        let mut accounts = Vec::new();

        for (addr_str, genesis_account) in &self.alloc {
            // Remove 0x prefix if present
            let addr_str = addr_str.trim_start_matches("0x");

            let address = Address::from_str_hex(addr_str)?;

            // Create the account
            let mut account = Account::new_empty(address);

            // Convert balance
            let balance_str = genesis_account.balance.trim_start_matches("0x");

            let balance = if genesis_account.balance.starts_with("0x") {
                // For hex strings
                Wei::from_str_hex(balance_str)?
            } else {
                // For decimal strings
                let value = balance_str.parse::<u64>().unwrap_or(0);
                Wei::from(EthereumU256::from(value))
            };

            account.balance = balance;

            // Add nonce if it exists
            if let Some(nonce) = &genesis_account.nonce {
                let nonce_str = nonce.trim_start_matches("0x");
                let nonce_value = if nonce.starts_with("0x") {
                    u64::from_str_radix(nonce_str, 16).unwrap_or(0)
                } else {
                    nonce_str.parse::<u64>().unwrap_or(0)
                };
                account.nonce = Nonce::from(nonce_value);
            }

            // Add code if it exists
            if let Some(code) = &genesis_account.code {
                let code_str = code.trim_start_matches("0x");
                if let Ok(code_bytes) = hex::decode(code_str) {
                    if !code_bytes.is_empty() {
                        account.bytecode = Some(revm::primitives::Bytecode::new_raw(code_bytes.into()));
                    }
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
        // Ensure the hex string has an even number of digits
        let hex = if hex.len() % 2 == 1 {
            format!("0{}", hex) // Add a leading zero if needed
        } else {
            hex.to_string()
        };

        if hex.len() != 40 {
            return Err(anyhow!("Invalid address length: {}", hex.len()));
        }

        let bytes = match hex::decode(&hex) {
            Ok(b) => b,
            Err(e) => return Err(anyhow!("Failed to decode address: {}", e)),
        };

        if bytes.len() != 20 {
            return Err(anyhow!("Unexpected bytes length: {}", bytes.len()));
        }

        let mut array = [0u8; 20];
        array.copy_from_slice(&bytes);
        Ok(Address::new(array))
    }
}

impl FromStrHex for Wei {
    fn from_str_hex(hex: &str) -> Result<Self> {
        // Ensure the hex string has an even number of digits
        let hex = if hex.len() % 2 == 1 {
            format!("0{}", hex) // Add a leading zero if needed
        } else {
            hex.to_string()
        };

        // Decode the hex string
        let bytes = match hex::decode(&hex) {
            Ok(b) => b,
            Err(e) => return Err(anyhow!("Failed to decode hex string: {}", e)),
        };

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
    use super::*;

    #[test]
    fn test_load_genesis_and_convert_accounts() {
        use std::fs::File;
        use std::io::Write;

        // Criar um arquivo temporário sem usar tempfile
        let file_path = "test_genesis.json";

        // Create a minimal genesis.json file
        let json = r#"
        {
          "config": {
            "chainId": 2008
          },
          "nonce": "0x0",
          "timestamp": "0x0",
          "extraData": "0x0",
          "gasLimit": "0x0",
          "difficulty": "0x0",
          "mixHash": "0x0",
          "coinbase": "0x0000000000000000000000000000000000000000",
          "alloc": {
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266": {
              "balance": "0x1000",
              "nonce": "0x1"
            }
          }
        }"#;

        // Write the JSON to the file
        let mut file = File::create(file_path).unwrap();
        file.write_all(json.as_bytes()).unwrap();

        // Load the genesis config from the file
        let genesis = GenesisConfig::load_from_file(file_path).unwrap();

        // Test address conversion
        let addr_str = "f39fd6e51aad88f6f4ce6ab8827279cfffb92266";
        let addr = Address::from_str_hex(addr_str).unwrap();

        // Convert accounts
        let accounts = genesis.to_stratus_accounts().unwrap();

        // Verify the account
        assert_eq!(accounts.len(), 1);
        let account = &accounts[0];
        assert_eq!(account.address, addr);
        assert_eq!(account.nonce, Nonce::from(1u64));
        assert_eq!(account.balance, Wei::from(EthereumU256::from(4096))); // 0x1000 in decimal

        // Clean up
        std::fs::remove_file(file_path).unwrap();
    }
}
