import { network } from "hardhat";
import { match } from "ts-pattern";

export enum Network {
    Stratus = "stratus",
    Hardhat = "hardhat",
    Anvil = "anvil",
    Unknown = "",
}

export function currentNetwork(): Network {
    switch (network.name) {
        case "stratus":
            return Network.Stratus;
        case "hardhat":
            return Network.Hardhat;
        case "anvil":
            return Network.Anvil;
        default:
            return Network.Unknown;
    }
}
export const isStratus = currentNetwork() == Network.Stratus;
