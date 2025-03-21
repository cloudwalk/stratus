import { ethers } from "ethers";
import { existsSync, readFileSync } from "fs";
import { join } from "path";

/**
 * Interface representing a complete account from the genesis file
 */
export interface GenesisAccount {
    balance: string;
    code?: string;
    nonce?: string;
    storage?: Record<string, string>;
}

/**
 * Helper function to load and validate a genesis configuration file
 * @param path Path to the genesis file
 * @returns The parsed genesis configuration
 */
export function loadGenesisFile(path: string = "../config/genesis.local.json") {
    // Use the provided path or resolve with project root
    const fullPath = path.startsWith("/") ? path : join(process.cwd(), path);

    if (!existsSync(fullPath)) {
        throw new Error(`Genesis file not found at ${fullPath}`);
    }

    try {
        const content = readFileSync(fullPath, "utf8");
        return JSON.parse(content);
    } catch (error) {
        throw new Error(`Failed to parse genesis file: ${error}`);
    }
}

/**
 * Helper function to get accounts with their expected balances from a genesis file
 * @param path Path to the genesis file
 * @returns Map of addresses to their balance
 */
export function getGenesisAccountBalances(path: string = "../config/genesis.local.json"): Map<string, string> {
    const genesis = loadGenesisFile(path);
    const accountBalances = new Map<string, string>();
    
    for (const [address, details] of Object.entries(genesis.alloc)) {
        accountBalances.set(address, (details as any).balance);
    }
    
    return accountBalances;
}

/**
 * Helper function to get full account details from a genesis file
 * @param path Path to the genesis file
 * @returns Map of addresses to their complete account details
 */
export function getGenesisAccounts(path: string = "../config/genesis.local.json"): Map<string, GenesisAccount> {
    const genesis = loadGenesisFile(path);
    const accounts = new Map<string, GenesisAccount>();
    
    for (const [address, details] of Object.entries(genesis.alloc)) {
        const account: GenesisAccount = { 
            balance: (details as any).balance 
        };
        
        if ((details as any).code) {
            account.code = (details as any).code;
        }
        
        if ((details as any).nonce) {
            account.nonce = (details as any).nonce;
        }
        
        if ((details as any).storage) {
            account.storage = (details as any).storage;
        }
        
        accounts.set(address, account);
    }
    
    return accounts;
}

/**
 * Helper function to get accounts with their expected balances from a genesis file as tuples
 * @param path Path to the genesis file
 * @returns Array of [address, balance] tuples
 */
export function getGenesisAccountTuples(path: string = "../config/genesis.local.json"): Array<[string, ethers.BigNumberish]> {
    const genesis = loadGenesisFile(path);

    return Object.entries(genesis.alloc).map(([address, account]: [string, any]) => {
        // Convert hex balance to BigNumber
        const balance = ethers.parseUnits(account.balance, 0);
        return [address, balance];
    });
}
