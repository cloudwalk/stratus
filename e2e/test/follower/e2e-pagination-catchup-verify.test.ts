import axios from "axios";
import { expect } from "chai";
import fs from "fs";

// Part 2 of the catch-up scenario, executed by the `just e2e-leader-follower-pagination-catchup`
// recipe AFTER the follower starts: the follower was several fat blocks behind (see the setup
// step) and must catch up through all the paginated responses, then serve them correctly.

const FOLLOWER_URL = process.env.FOLLOWER_URL || "http://localhost:3001";
const LEADER_URL = process.env.LEADER_URL || "http://localhost:3000";
const ARTIFACT_FILE = "pagination-catchup.json";
const FOLLOWER_SYNC_TIMEOUT_MS = 120_000;
const POLL_INTERVAL_MS = 500;

describe("Pagination catch-up (verify)", () => {
    it("catches up through multiple paginated fat blocks", async () => {
        const artifact = JSON.parse(fs.readFileSync(ARTIFACT_FILE, "utf-8"));
        expect(artifact.blocks.length).to.be.greaterThan(1, "the setup step must have mined several fat blocks");

        // the follower started behind and must reach the fat blocks mined while it was down
        await waitForFollowerBlock(artifact.target);

        // every fat block was imported: each fat receipt is served with the right block number
        for (const block of artifact.blocks) {
            const receipt = await rpcCall(FOLLOWER_URL, "eth_getTransactionReceipt", [block.txHash]);
            expect(receipt.result).to.not.be.null;
            expect(parseInt(receipt.result.blockNumber, 16)).to.equal(block.blockNumber);
        }

        // the last fat block content matches the leader
        const last = artifact.blocks[artifact.blocks.length - 1];
        const leaderBlock = await rpcCall(LEADER_URL, "eth_getBlockByNumber", [toHexNumber(last.blockNumber), false]);
        const followerBlock = await rpcCall(FOLLOWER_URL, "eth_getBlockByNumber", [
            toHexNumber(last.blockNumber),
            false,
        ]);
        expect(followerBlock.result.hash).to.equal(leaderBlock.result.hash);
    });
});

// Sends a JSON-RPC request to an arbitrary node, tolerating error statuses.
async function rpcCall(url: string, method: string, params: any[] = []): Promise<any> {
    const response = await axios.post(
        url,
        { jsonrpc: "2.0", id: 1, method: method, params: params },
        { validateStatus: () => true },
    );
    return response.data;
}

// Polls the follower until it reaches the target block number.
async function waitForFollowerBlock(target: number): Promise<void> {
    const deadline = Date.now() + FOLLOWER_SYNC_TIMEOUT_MS;
    while (Date.now() < deadline) {
        const body = await rpcCall(FOLLOWER_URL, "eth_blockNumber", []);
        const current = parseInt(body.result, 16);
        if (current >= target) {
            return;
        }
        await sleep(POLL_INTERVAL_MS);
    }
    throw new Error(`follower did not reach block ${target} in time`);
}

function sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

function toHexNumber(value: number): string {
    return "0x" + value.toString(16);
}
