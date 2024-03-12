import { HardhatUserConfig } from "hardhat/config";
import "@nomicfoundation/hardhat-toolbox";
import '@openzeppelin/hardhat-upgrades';

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
      accounts: {
        mnemonic: "test test test test test test test test test test test junk",
        accountsBalance: "18446744073709551615", // u64::max
      },
    },
  },
};

export default config;
