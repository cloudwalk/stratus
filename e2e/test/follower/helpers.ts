import axios from "axios";

import { send } from "../helpers/rpc";

// Follower and leader URLs, matching the e2e recipes.
export const FOLLOWER_URL = process.env.FOLLOWER_URL || "http://localhost:3001";
export const LEADER_URL = process.env.LEADER_URL || "http://localhost:3000";

// Sends a JSON-RPC request to an arbitrary node, tolerating error statuses.
export async function rpcCall(url: string, method: string, params: any[] = []): Promise<any> {
    const response = await axios.post(
        url,
        { jsonrpc: "2.0", id: 1, method: method, params: params },
        { validateStatus: () => true },
    );
    return response.data;
}

// Polls the leader until the transaction is mined and returns its receipt.
export async function waitForReceipt(txHash: string): Promise<any> {
    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline) {
        const receipt = await send("eth_getTransactionReceipt", [txHash]);
        if (receipt) {
            return receipt;
        }
        await sleep(500);
    }
    throw new Error(`transaction ${txHash} was not mined in time`);
}

// Polls the follower until it reaches the target block number.
export async function waitForFollowerBlock(target: number): Promise<void> {
    const deadline = Date.now() + 120_000;
    while (Date.now() < deadline) {
        const body = await rpcCall(FOLLOWER_URL, "eth_blockNumber", []);
        const current = parseInt(body.result, 16);
        if (current >= target) {
            return;
        }
        await sleep(500);
    }
    throw new Error(`follower did not reach block ${target} in time`);
}

export function sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

export function toHexNumber(value: number): string {
    return "0x" + value.toString(16);
}
