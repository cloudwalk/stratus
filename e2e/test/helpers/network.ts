import { network } from "hardhat";

export const MINING_INTERVAL_PATTERN = /^(\d+)s$/;

export enum Network {
    Stratus = "stratus",
    Hardhat = "hardhat",
    Unknown = "",
}

export enum BlockMode {
    Automine = "automine",
    External = "external",
    Interval = "interval"
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
    if (process.env.BLOCK_MODE === "external") {
        return 0;
    } else {
        const regexpResults = MINING_INTERVAL_PATTERN.exec(process.env.BLOCK_MODE ?? "");
        if (regexpResults && regexpResults.length > 1) {
            return parseInt(regexpResults[1]) * 1000;
        }
    }
    return undefined;
}