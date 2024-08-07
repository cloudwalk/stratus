import "@nomicfoundation/hardhat-toolbox";
import "@nomicfoundation/hardhat-toolbox";
import "@openzeppelin/hardhat-upgrades";
import { HardhatUserConfig } from "hardhat/config";

const ACCOUNTS_MNEMONIC = "test test test test test test test test test test test junk";

const STRATUS_PORT = process.env.STRATUS_PORT || 3000; // Default to 3000 if not set

const url = `http://0.0.0.0:${STRATUS_PORT}?app=e2e`;

const config: HardhatUserConfig = {
    defaultNetwork: "hardhat",
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
            initialBaseFeePerGas: 0,
            mining: {
                auto: process.env.BLOCK_MODE === "automine",
                interval:
                    process.env.BLOCK_MODE === "automine"
                        ? undefined
                        : process.env.BLOCK_MODE === "1s"
                          ? 1000
                          : Number(process.env.BLOCK_MODE),
            },
            accounts: {
                mnemonic: ACCOUNTS_MNEMONIC,
                accountsBalance: "18446744073709551615", // u64::max
            },
        },
        stratus: {
            url: url,
            accounts: {
                mnemonic: ACCOUNTS_MNEMONIC,
            },
            timeout: 40000,
        },
    },
};

export default config;
