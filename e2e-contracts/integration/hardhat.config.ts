import "@nomicfoundation/hardhat-toolbox";
import "@nomicfoundation/hardhat-toolbox";
import "@openzeppelin/hardhat-upgrades";
import { HardhatUserConfig } from "hardhat/config";

const ACCOUNTS_MNEMONIC = "test test test test test test test test test test test junk";

const config: HardhatUserConfig = {
    defaultNetwork: "hardhat",
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
            initialBaseFeePerGas: 0,
            mining: {
                auto: process.env.BLOCK_MODE === 'automine',
                interval: process.env.BLOCK_MODE === 'automine' ? undefined : (process.env.BLOCK_MODE === '1s' ? 1000 : 0)
            },
            accounts: {
                mnemonic: ACCOUNTS_MNEMONIC,
                accountsBalance: "18446744073709551615", // u64::max
            },
        },
        stratus: {
            url: "http://localhost:3000?app=e2e",
            accounts: {
                mnemonic: ACCOUNTS_MNEMONIC,
            },
            timeout: 40000,
        },
    },
};

export default config;
