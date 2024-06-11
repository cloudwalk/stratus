import { network } from "hardhat";

import { defineBlockMiningIntervalInMs } from "../../hardhat.config";

export enum Network {
    Stratus = "stratus",
    Hardhat = "hardhat",
    Unknown = "",
}

export enum BlockMode {
    Automine = "automine",
    External = "external",
    Interval = "interval",
}

export function currentNetwork(): Network {
    switch (network.name) {
        case "stratus":
            return Network.Stratus;
        case "hardhat":
            return Network.Hardhat;
        default:
            return Network.Unknown;
    }
}

export const isStratus = currentNetwork() == Network.Stratus;

export function currentBlockMode(): BlockMode {
    if (process.env.BLOCK_MODE) {
        switch (process.env.BLOCK_MODE) {
            case "automine":
                return BlockMode.Automine;
            case "external":
                return BlockMode.External;
            default:
                return BlockMode.Interval;
        }
    } else {
        return BlockMode.Automine;
    }
}

export function currentMiningIntervalInMs(): number | undefined {
    return defineBlockMiningIntervalInMs(process.env.BLOCK_MODE);
}
