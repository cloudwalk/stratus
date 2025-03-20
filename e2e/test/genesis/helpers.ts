import { existsSync, readFileSync, writeFileSync } from 'fs';
import { join } from 'path';
import { ethers } from 'ethers';

/**
 * Create a custom genesis file for testing purposes
 * @param accounts Array of account addresses to allocate funds
 * @param balance Amount in wei to allocate to each account
 * @param path Path where the genesis file should be saved
 * @returns The full path to the created genesis file
 */
export function createCustomGenesisFile(
  accounts: string[] = [],
  balance: string = "0xffffffffffffffff",
  path: string = 'config/genesis.test.json'
): string {
  const fullPath = join(process.cwd(), '../../', path);
  
  // Create a basic genesis config
  const genesis = {
    config: {
      chainId: 2008,
      homesteadBlock: 0,
      eip150Block: 0,
      eip155Block: 0,
      eip158Block: 0,
      byzantiumBlock: 0,
      constantinopleBlock: 0,
      petersburgBlock: 0,
      istanbulBlock: 0,
      berlinBlock: 0,
      londonBlock: 0,
      shanghaiTime: 0,
      cancunTime: 0
    },
    nonce: "0x0000000000000042",
    timestamp: "0x0",
    extraData: "0x0000000000000000000000000000000000000000000000000000000000000000",
    gasLimit: "0xffffffff",
    difficulty: "0x1",
    mixHash: "0x0000000000000000000000000000000000000000000000000000000000000000",
    coinbase: "0x0000000000000000000000000000000000000000",
    alloc: {},
    number: "0x0",
    gasUsed: "0x0",
    parentHash: "0x0000000000000000000000000000000000000000000000000000000000000000",
    baseFeePerGas: "0x0"
  };
  
  // Allocate funds to accounts
  const alloc: Record<string, { balance: string }> = {};
  for (const account of accounts) {
    alloc[account] = { balance };
  }
  
  // Add the allocations
  genesis.alloc = alloc;
  
  // Write to file
  writeFileSync(fullPath, JSON.stringify(genesis, null, 2));
  
  return fullPath;
}

/**
 * Helper function to load and validate a genesis configuration file
 * @param path Path to the genesis file
 * @returns The parsed genesis configuration
 */
export function loadGenesisFile(path: string = 'config/genesis.local.json') {
  const fullPath = join(process.cwd(), '../../', path);
  
  if (!existsSync(fullPath)) {
    throw new Error(`Genesis file not found at ${fullPath}`);
  }
  
  try {
    const content = readFileSync(fullPath, 'utf8');
    return JSON.parse(content);
  } catch (error) {
    throw new Error(`Failed to parse genesis file: ${error}`);
  }
}

/**
 * Helper function to get accounts with their expected balances from a genesis file
 * @param path Path to the genesis file
 * @returns Array of [address, balance] tuples
 */
export function getGenesisAccounts(path: string = './config/genesis.local.json'): Array<[string, ethers.BigNumberish]> {
  const genesis = loadGenesisFile(path);
  
  return Object.entries(genesis.alloc).map(([address, account]: [string, any]) => {
    // Convert hex balance to BigNumber
    const balance = ethers.parseUnits(account.balance, 0);
    return [address, balance];
  });
} 