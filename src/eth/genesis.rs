use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::str::FromStr;

use alloy_primitives::B64;
use alloy_primitives::FixedBytes;
use alloy_primitives::U256;
use alloy_primitives::hex;
use anyhow::Result;
use const_hex::FromHex;
use serde::Deserialize;
use serde::Serialize;

use crate::alias::RevmBytecode;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::Wei;

/// Type alias for a collection of storage slots in the format (address, slot)
pub type GenesisSlots = Vec<(Address, Slot)>;

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
    /// Loads a genesis configuration from a file.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let genesis: GenesisConfig = serde_json::from_reader(reader)?;
        Ok(genesis)
    }

    /// Converts the genesis configuration to Stratus accounts and storage slots.
    ///
    /// Returns a tuple containing:
    /// - A vector of accounts with their balance, nonce, and bytecode
    /// - A vector of storage slots in the format (address, slot_index, value)
    pub fn to_stratus_accounts_and_slots(&self) -> Result<(Vec<Account>, GenesisSlots)> {
        let mut accounts = Vec::new();
        let mut slots = Vec::new();

        for (addr_str, genesis_account) in &self.alloc {
            // Remove 0x prefix if present
            let addr_str = addr_str.trim_start_matches("0x");

            // Use FromStr to convert the address
            let address = Address::from_str(addr_str)?;

            // Create the account
            let mut account = Account::new_empty(address);

            // Convert balance
            let balance = if genesis_account.balance.starts_with("0x") {
                // For hex strings
                Wei::from_hex_str(&genesis_account.balance)?
            } else {
                // For decimal strings
                let value = genesis_account.balance.parse::<u64>().unwrap_or(0);
                Wei::from(U256::from(value))
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
                if let Ok(code_bytes) = hex::decode(code_str)
                    && !code_bytes.is_empty()
                {
                    account.bytecode = Some(RevmBytecode::new_raw(code_bytes.into()));
                }
            }

            // Process storage slots if they exist
            if let Some(storage) = &genesis_account.storage {
                for (slot_key, slot_value) in storage {
                    // Parse slot key and value in just two lines
                    let slot_index: SlotIndex = FixedBytes::<32>::from_hex(slot_key)?.into();
                    let slot_value: SlotValue = FixedBytes::<32>::from_hex(slot_value)?.into();

                    // Add slot to the list
                    slots.push((address, Slot::new(slot_index, slot_value)));
                }
            }

            accounts.push(account);
        }
        Ok((accounts, slots))
    }

    /// Converts the genesis configuration to Stratus accounts.
    ///
    /// This is a convenience wrapper around to_stratus_accounts_and_slots that only returns the accounts.
    pub fn to_stratus_accounts(&self) -> Result<Vec<Account>> {
        let (accounts, _) = self.to_stratus_accounts_and_slots()?;
        Ok(accounts)
    }

    /// Creates a genesis block from the genesis configuration.
    pub fn to_genesis_block(&self) -> Result<crate::eth::primitives::Block> {
        use std::str::FromStr;

        use alloy_primitives::hex;

        use crate::eth::primitives::Block;
        use crate::eth::primitives::BlockHeader;
        use crate::eth::primitives::BlockNumber;
        use crate::eth::primitives::Bytes;
        use crate::eth::primitives::Gas;
        use crate::eth::primitives::Hash;
        use crate::eth::primitives::UnixTime;
        // Parse timestamp
        let timestamp_str = self.timestamp.trim_start_matches("0x");
        let timestamp = if self.timestamp.starts_with("0x") {
            u64::from_str_radix(timestamp_str, 16).unwrap_or(0)
        } else {
            timestamp_str.parse::<u64>().unwrap_or(0)
        };

        // Parse gas limit
        let gas_limit_str = self.gasLimit.trim_start_matches("0x");
        let gas_limit = if self.gasLimit.starts_with("0x") {
            u64::from_str_radix(gas_limit_str, 16).unwrap_or(0)
        } else {
            gas_limit_str.parse::<u64>().unwrap_or(0)
        };

        // Parse gas used (if present)
        let gas_used = if let Some(gas_used) = &self.gasUsed {
            let gas_used_str = gas_used.trim_start_matches("0x");
            if gas_used.starts_with("0x") {
                u64::from_str_radix(gas_used_str, 16).unwrap_or(0)
            } else {
                gas_used_str.parse::<u64>().unwrap_or(0)
            }
        } else {
            0
        };

        // Parse nonce
        let nonce_str = self.nonce.trim_start_matches("0x");
        let nonce_bytes = if let Ok(bytes) = hex::decode(nonce_str) {
            let mut padded = [0u8; 8];
            let start = padded.len().saturating_sub(bytes.len());
            let copy_len = bytes.len().min(padded.len() - start);
            padded[start..start + copy_len].copy_from_slice(&bytes[..copy_len]);
            padded
        } else {
            [0u8; 8]
        };

        // Parse coinbase/miner address
        let miner = Address::from_str(&self.coinbase).unwrap_or_default();

        // Parse extra data
        let extra_data_str = self.extraData.trim_start_matches("0x");
        let extra_data = if let Ok(bytes) = hex::decode(extra_data_str) {
            Bytes(bytes)
        } else {
            Bytes(vec![])
        };

        // Parse parent hash (if present)
        let parent_hash = if let Some(parent_hash) = &self.parentHash {
            let parent_hash_str = parent_hash.trim_start_matches("0x");
            if let Ok(bytes) = hex::decode(parent_hash_str) {
                let mut hash_bytes = [0u8; 32];
                let start = hash_bytes.len().saturating_sub(bytes.len());
                let copy_len = bytes.len().min(hash_bytes.len() - start);
                hash_bytes[start..start + copy_len].copy_from_slice(&bytes[..copy_len]);
                Hash::new(hash_bytes)
            } else {
                Hash::default()
            }
        } else {
            Hash::default()
        };

        // Parse mix hash
        let mix_hash_str = self.mixHash.trim_start_matches("0x");
        let _mix_hash = if let Ok(bytes) = hex::decode(mix_hash_str) {
            let mut hash_bytes = [0u8; 32];
            let start = hash_bytes.len().saturating_sub(bytes.len());
            let copy_len = bytes.len().min(hash_bytes.len() - start);
            hash_bytes[start..start + copy_len].copy_from_slice(&bytes[..copy_len]);
            Hash::new(hash_bytes)
        } else {
            Hash::default()
        };

        // Parse difficulty
        let difficulty = if self.difficulty.starts_with("0x") {
            u64::from_str_radix(self.difficulty.trim_start_matches("0x"), 16).unwrap_or(0)
        } else {
            self.difficulty.parse::<u64>().unwrap_or(0)
        };

        // Create a basic header
        let mut header = BlockHeader::new(BlockNumber::ZERO, UnixTime::from(timestamp));

        // Update with genesis.json values
        header.gas_limit = Gas::from(gas_limit);
        header.gas_used = Gas::from(gas_used);
        header.miner = miner;
        header.author = miner;
        header.extra_data = extra_data;
        header.parent_hash = parent_hash;
        header.difficulty = difficulty.into();

        // For nonce, we need to convert to B64 first
        let nonce_b64 = B64::from_slice(&nonce_bytes);
        header.nonce = nonce_b64.into();

        // Create the block
        let block = Block {
            header,
            transactions: Vec::new(),
        };

        Ok(block)
    }
}

impl Default for GenesisConfig {
    #[cfg(feature = "dev")]
    fn default() -> Self {
        // Default implementation for development environment
        let mut alloc = BTreeMap::new();

        // Add test accounts
        let test_accounts = crate::eth::primitives::test_accounts();
        for account in test_accounts {
            let address_str = format!("0x{}", hex::encode(account.address.as_slice()));
            let balance_hex = format!("0x{:x}", account.balance.0);

            let mut genesis_account = GenesisAccount {
                balance: balance_hex,
                code: None,
                nonce: Some(format!("0x{:x}", u64::from(account.nonce))),
                storage: None,
            };

            // Add code if not empty
            if let Some(ref bytecode) = account.bytecode
                && !bytecode.is_empty()
            {
                genesis_account.code = Some(format!("0x{}", hex::encode(bytecode.bytes())));
            }

            alloc.insert(address_str, genesis_account);
        }

        Self {
            config: ChainConfig {
                chainId: 2008,
                homesteadBlock: Some(0),
                eip150Block: Some(0),
                eip155Block: Some(0),
                eip158Block: Some(0),
                byzantiumBlock: Some(0),
                constantinopleBlock: Some(0),
                petersburgBlock: Some(0),
                istanbulBlock: Some(0),
                berlinBlock: Some(0),
                londonBlock: Some(0),
                shanghaiTime: Some(0),
                cancunTime: Some(0),
                pragueTime: None,
            },
            nonce: "0x0000000000000042".to_string(),
            timestamp: "0x0".to_string(),
            extraData: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            gasLimit: "0xffffffff".to_string(),
            difficulty: "0x1".to_string(),
            mixHash: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            coinbase: "0x0000000000000000000000000000000000000000".to_string(),
            alloc,
            number: Some("0x0".to_string()),
            gasUsed: Some("0x0".to_string()),
            parentHash: Some("0x0000000000000000000000000000000000000000000000000000000000000000".to_string()),
            baseFeePerGas: Some("0x0".to_string()),
        }
    }

    #[cfg(not(feature = "dev"))]
    fn default() -> Self {
        // Minimal default implementation for non-development environments
        Self {
            config: ChainConfig {
                chainId: 2008,
                homesteadBlock: None,
                eip150Block: None,
                eip155Block: None,
                eip158Block: None,
                byzantiumBlock: None,
                constantinopleBlock: None,
                petersburgBlock: None,
                istanbulBlock: None,
                berlinBlock: None,
                londonBlock: None,
                shanghaiTime: None,
                cancunTime: None,
                pragueTime: None,
            },
            nonce: "0x0".to_string(),
            timestamp: "0x0".to_string(),
            extraData: "0x0".to_string(),
            gasLimit: "0x0".to_string(),
            difficulty: "0x0".to_string(),
            mixHash: "0x0".to_string(),
            coinbase: "0x0000000000000000000000000000000000000000".to_string(),
            alloc: BTreeMap::new(),
            number: None,
            gasUsed: None,
            parentHash: None,
            baseFeePerGas: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_genesis_and_convert_accounts() {
        use std::fs::File;
        use std::io::Write;

        // Create a temporary file without using tempfile
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
        let addr = Address::from_str(addr_str).unwrap();

        // Convert accounts
        let accounts = genesis.to_stratus_accounts().unwrap();

        // Verify the account
        assert_eq!(accounts.len(), 1);
        let account = &accounts[0];
        assert_eq!(account.address, addr);
        assert_eq!(account.nonce, Nonce::from(1u64));
        assert_eq!(account.balance, Wei::from(U256::from(4096))); // 0x1000 in decimal

        // Clean up
        std::fs::remove_file(file_path).unwrap();
    }

    #[test]
    fn test_default_genesis_config() {
        // Test the default implementation
        let default_genesis = GenesisConfig::default();

        // Verify chain ID is set to 2008
        assert_eq!(default_genesis.config.chainId, 2008);

        #[cfg(feature = "dev")]
        {
            // In dev mode, we should have test accounts
            let test_accounts = crate::eth::primitives::test_accounts();
            let stratus_accounts = default_genesis.to_stratus_accounts().unwrap();

            // Verify the number of accounts matches
            assert_eq!(stratus_accounts.len(), test_accounts.len());

            // Verify each account exists in the genesis config
            for test_account in test_accounts {
                let found = stratus_accounts.iter().any(|account| account.address == test_account.address);
                assert!(found, "Test account not found in default genesis config");
            }
        }

        #[cfg(not(feature = "dev"))]
        {
            // In non-dev mode, we should have an empty alloc
            assert!(default_genesis.alloc.is_empty());
        }
    }

    #[test]
    fn test_genesis_file_with_clap_integration() {
        use std::env;

        use clap::Parser;

        use crate::config::GenesisFileConfig;

        // Path to the local genesis file
        let file_path = "config/genesis.local.json";

        // Test 1: Direct file loading
        let genesis = GenesisConfig::load_from_file(file_path).expect("Failed to load genesis.local.json");

        // Verify basic properties
        assert_eq!(genesis.config.chainId, 2008);

        // Verify accounts
        let accounts = genesis.to_stratus_accounts().expect("Failed to convert accounts");
        assert!(!accounts.is_empty(), "No accounts found in genesis.local.json");

        // Verify the first account
        let first_account_addr = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
        let first_addr = Address::from_str(first_account_addr).unwrap();

        let found = accounts.iter().any(|account| account.address == first_addr);
        assert!(found, "First account not found in genesis.local.json");

        // Verify account balance
        if let Some(account) = accounts.iter().find(|account| account.address == first_addr) {
            assert_eq!(account.balance, Wei::TEST_BALANCE);
        }

        // Test 2: Clap integration - command line arguments
        let args = vec!["program", "--genesis-path", file_path];
        let config = GenesisFileConfig::parse_from(args);
        assert_eq!(config.genesis_path, Some(file_path.to_string()));

        // Load the file using the path obtained via clap
        let genesis_from_clap = GenesisConfig::load_from_file(config.genesis_path.unwrap()).expect("Failed to load genesis.local.json via clap");

        // Verify that the file loaded via clap has the same chainId
        assert_eq!(genesis_from_clap.config.chainId, 2008);

        // Test 3: Clap integration - environment variables
        unsafe {
            env::set_var("GENESIS_JSON_PATH", file_path);
        }
        let args = vec!["program"]; // No command line arguments
        let config = GenesisFileConfig::parse_from(args);
        assert_eq!(config.genesis_path, Some(file_path.to_string()));

        // Load the file using the path obtained via environment variable
        let genesis_from_env = GenesisConfig::load_from_file(config.genesis_path.unwrap()).expect("Failed to load genesis.local.json via env var");

        // Verify that the file loaded via environment variable has the same chainId
        assert_eq!(genesis_from_env.config.chainId, 2008);

        // Clean up
        unsafe {
            env::remove_var("GENESIS_JSON_PATH");
        }
    }
}
