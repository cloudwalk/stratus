import { HardhatUserConfig } from "hardhat/config";
import "@nomicfoundation/hardhat-toolbox";

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

const config: HardhatUserConfig = {
    solidity: {
        version: "0.8.16",
        settings: {
            optimizer: {
                enabled: true,
                runs: 1000,
            },
        },
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
            url: "http://localhost:3000",
            accounts: {
                mnemonic: ACCOUNTS_MNEMONIC,
            },
            timeout: 40000,
        },
    },
};

export default config;
