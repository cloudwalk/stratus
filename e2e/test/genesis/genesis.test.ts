import { expect } from "chai";
import { existsSync, readFileSync } from "fs";
import { ethers } from "hardhat";
import { join } from "path";

import { GenesisAccount, getGenesisAccounts } from "./helpers";

describe("Genesis configuration", () => {
    // Dynamically load accounts and balances from genesis file
    const genesisFilePath = process.env.GENESIS_PATH || "../config/genesis.local.json";
    const fullGenesisPath = genesisFilePath.startsWith("/") ? genesisFilePath : join(process.cwd(), genesisFilePath);

    // Load genesis config directly
    let genesisContent: any;
    let genesisAccounts: string[] = [];

    // Map for storing account data
    let accountsData: Map<string, GenesisAccount> = new Map();

    before(async function () {
        try {
            // Read and parse the genesis file
            if (!existsSync(fullGenesisPath)) {
                console.error(`Genesis file not found at ${fullGenesisPath}`);
                this.skip();
                return;
            }

            const content = readFileSync(fullGenesisPath, "utf8");
            genesisContent = JSON.parse(content);

            // Get accounts data using our helper
            accountsData = getGenesisAccounts(genesisFilePath);
            genesisAccounts = Array.from(accountsData.keys());

            if (genesisAccounts.length === 0) {
                console.error("No accounts found in genesis file");
                this.skip();
            }
        } catch (error) {
            console.error("Error loading genesis file:", error);
            this.skip();
        }
    });

    it("should allocate correct balances to genesis accounts", async () => {
        // Verify each account has the expected balance
        for (const account of genesisAccounts) {
            const balance = await ethers.provider.getBalance(account);
            // Get the expected balance for this specific account
            const accountData = accountsData.get(account)!;
            // Use BigInt directly instead of parseUnits for hex values
            const expectedBalance = BigInt(accountData.balance);

            expect(balance).to.equal(expectedBalance);
        }
    });

    it("should allocate correct nonce to genesis accounts", async () => {
        // Verify each account has the expected balance
        for (const account of genesisAccounts) {
            const nonce = await ethers.provider.getTransactionCount(account, 0);
            // Get the expected balance for this specific account
            const accountData = accountsData.get(account)!;
            // Convert the hex nonce to a number
            const expectedNonce = accountData.nonce ? parseInt(accountData.nonce, 16) : 0;
            expect(nonce).to.equal(expectedNonce);
        }
    });
});
