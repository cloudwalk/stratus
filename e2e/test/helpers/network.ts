import { network } from "hardhat";
import { match } from "ts-pattern";

export enum Network {
    Ledger = "ledger",
    Hardhat = "hardhat",
    Anvil = "anvil",
    Unknown = "",
}

export const NETWORK = match(network.name)
    .returnType<Network>()
    .with("hardhat", () => Network.Hardhat)
    .with("anvil", () => Network.Anvil)
    .with("ledger", () => Network.Ledger)
    .otherwise(() => Network.Unknown);
