import { network } from "hardhat";
import { match } from "ts-pattern";

export enum Network {
    Stratus = "stratus",
    Hardhat = "hardhat",
    Anvil = "anvil",
    Unknown = "",
}

export const CURRENT_NETWORK = match(network.name)
    .returnType<Network>()
    .with("hardhat", () => Network.Hardhat)
    .with("anvil", () => Network.Anvil)
    .with("stratus", () => Network.Stratus)
    .otherwise(() => Network.Unknown);
