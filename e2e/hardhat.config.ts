import "@nomicfoundation/hardhat-toolbox";
import "@openzeppelin/hardhat-upgrades";
import { HardhatUserConfig } from "hardhat/config";

const ACCOUNTS_MNEMONIC = "test test test test test test test test test test test junk";
const MINING_INTERVAL_PATTERN = /^(\d+)s$/;

export function defineBlockMiningIntervalInMs(blockMintingModeTitle?: string): number | undefined {
    if (blockMintingModeTitle === "external") {
        return 0;
    } else {
        const regexpResults = MINING_INTERVAL_PATTERN.exec(blockMintingModeTitle ?? "");
        if (regexpResults && regexpResults.length > 1) {
            return parseInt(regexpResults[1]) * 1000;
        }
    }
    return undefined;
}

const STRATUS_PORT = process.env.STRATUS_PORT || 3000; // Default to 3000 if not set

const config: HardhatUserConfig = {
    solidity: {
        compilers: [
            {
                version: "0.8.16",
                settings: {
                    optimizer: {
                        enabled: true,
                        runs: 1000,
                    },
                },
            },
            {
                version: "0.8.24",
                settings: {
                    optimizer: {
                        enabled: true,
                        runs: 1000,
                    },
                },
            },
        ],
    },
    networks: {
        hardhat: {
            chainId: 2008,
            gasPrice: 0,
            initialBaseFeePerGas: 0,
            mining: {
                auto: process.env.BLOCK_MODE === "automine",
                interval: defineBlockMiningIntervalInMs(process.env.BLOCK_MODE),
            },
            accounts: {
                mnemonic: ACCOUNTS_MNEMONIC,
                accountsBalance: "18446744073709551615", // u64::max
            },
        },
        stratus: {
            url: `http://localhost:${STRATUS_PORT}`,
            accounts: {
                mnemonic: ACCOUNTS_MNEMONIC,
            },
            timeout: 40000,
        },
    },
};

export default config;
