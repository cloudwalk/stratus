import "@nomicfoundation/hardhat-toolbox";
import { HardhatUserConfig } from "hardhat/config";

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
                auto: true,
            },
        },
        anvil: {
            url: "http://localhost:8546",
        },
        ledger: {
            url: "http://localhost:3000",
        },
    },
};

export default config;
