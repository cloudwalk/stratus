import { network } from "hardhat";

export enum Network {
    Stratus = "stratus",
    Hardhat = "hardhat",
    Unknown = "",
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
