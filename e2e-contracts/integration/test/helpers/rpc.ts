import { ContractTransactionReceipt, ContractTransactionResponse } from "ethers"

export async function waitReceipt(txResponsePromise: Promise<ContractTransactionResponse>): Promise<ContractTransactionReceipt> {
    const txReceipt = await txResponsePromise;
    return txReceipt.wait() as Promise<ContractTransactionReceipt>;
}