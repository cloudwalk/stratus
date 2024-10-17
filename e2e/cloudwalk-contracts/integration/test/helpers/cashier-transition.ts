import { BaseContract, BytesLike, ContractTransactionResponse } from "ethers";
import { ethers } from "hardhat";

export interface Cashier extends BaseContract {
    getAddress(): Promise<string>;
    cashIn(address: string, amount: number, tx_id: BytesLike): Promise<ContractTransactionResponse>;
    requestCashOutFrom(address: string, amount: number, tx_id: BytesLike): Promise<ContractTransactionResponse>;
    confirmCashOut(tx_id: BytesLike): Promise<ContractTransactionResponse>;
    grantRole(role: BytesLike, account: string): Promise<ContractTransactionResponse>;
    CASHIER_ROLE(): Promise<string>;
}
export type Cashier__factory = any;

export async function getCashierFactory(): Promise<Cashier__factory> {
    let factory: Cashier__factory;
    try {
        factory = await ethers.getContractFactory("Cashier");
    } catch (HardhatError) {
        factory = await ethers.getContractFactory("PixCashier");
    }

    return factory;
}
