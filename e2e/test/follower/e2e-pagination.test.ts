import axios from "axios";
import { expect } from "chai";

import { ALICE } from "../helpers/account";
import { CHAIN_ID_DEC, send, sendAndGetFullResponse } from "../helpers/rpc";

// This test only makes sense with a leader and a follower running with small response size limits:
// the leader with MAX_RESPONSE_SIZE_BYTES and the follower with EXTERNAL_RPC_MAX_RESPONSE_SIZE_BYTES.
// It is executed by the `just e2e-leader-follower-pagination` recipe.
//
// What it covers, in order:
// 1. Mines a block whose serialized `stratus_getBlockAndReceipts` response exceeds the limits.
// 2. Confirms the old single-parameter call fails with the oversized response error (-32008),
//    which is what would stall an importer before pagination existed.
// 3. Reassembles the response from the paginated envelope chunks and validates the content.
// 4. Waits for the follower importer to sync the fat block, proving the importer paginated it.

// Fat transaction payload: enough data for the block + receipts response to exceed the limits.
const FAT_TX_DATA_BYTES = 50_000;

// Small chunk budget to force several round trips during reassembly.
const CHUNK_BUDGET = 1024;

const FOLLOWER_URL = process.env.FOLLOWER_URL || "http://localhost:3001";
const LEADER_MAX_RESPONSE_BYTES = parseInt(process.env.MAX_RESPONSE_SIZE_BYTES || "8192");
const FOLLOWER_SYNC_TIMEOUT_MS = 120_000;
const POLL_INTERVAL_MS = 500;

describe("Pagination", () => {
    it("paginates oversized importer responses and keeps the follower syncing", async () => {
        // send a fat contract-deployment transaction and wait for it to be mined;
        // the deployed code always fails, but the fat transaction data still makes the block heavy
        const nonce = await send("eth_getTransactionCount", [ALICE.address]);
        const signedTx = await ALICE.signer().signTransaction({
            data: "0x" + "ab".repeat(FAT_TX_DATA_BYTES),
            chainId: CHAIN_ID_DEC,
            gasPrice: 0,
            gasLimit: 10_000_000,
            nonce: nonce,
        });
        const txHash = await send("eth_sendRawTransaction", [signedTx]);

        const receipt = await waitForReceipt(txHash);
        const fatBlockNumber = parseInt(receipt.blockNumber, 16);

        // the block hash fits in one response (hashes only), but the full response does not
        const fatBlock = await send("eth_getBlockByNumber", [toHexNumber(fatBlockNumber), false]);
        const fatBlockHash = fatBlock.hash;

        // old-style single-parameter call: the oversized response is rejected by the server,
        // which is exactly what would get an old importer stuck retrying forever
        const legacy = await sendAndGetFullResponse("stratus_getBlockAndReceipts", [fatBlockHash]);
        expect(legacy.data.error).to.not.be.undefined;
        expect(legacy.data.error.code).to.equal(-32008);

        // paginated reassembly
        let assembled = "";
        let total = 0;
        for (let offset = 0; total === 0 || assembled.length < total; offset = assembled.length) {
            const envelope = await send("stratus_getBlockAndReceipts", [
                fatBlockHash,
                { offset: offset, chunk_budget: CHUNK_BUDGET },
            ]);
            expect(envelope.__stratus_paginated__).to.not.be.undefined;
            total = envelope.__stratus_paginated__.total;
            expect(envelope.__stratus_paginated__.chunk.length).to.be.greaterThan(0);
            assembled += envelope.__stratus_paginated__.chunk;
        }
        expect(assembled.length).to.equal(total);
        expect(total).to.be.greaterThan(LEADER_MAX_RESPONSE_BYTES, "the block response should be oversized");

        // reassembled content matches the actual block
        const response = JSON.parse(assembled);
        expect(response.block.hash).to.equal(fatBlockHash);
        expect(parseInt(response.block.number, 16)).to.equal(fatBlockNumber);
        expect(response.block.transactions).to.have.length(1);
        expect(response.receipts).to.have.length(1);
        expect(response.receipts[0].transactionHash).to.equal(txHash);

        // the follower imports the fat block through the paginated importer
        await waitForFollowerBlock(fatBlockNumber);
        const followerReceipt = await rpcCall(FOLLOWER_URL, "eth_getTransactionReceipt", [txHash]);
        expect(followerReceipt.result).to.not.be.null;
        expect(followerReceipt.result.blockNumber).to.equal(receipt.blockNumber);
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

// Polls the leader until the transaction is mined and returns its receipt.
async function waitForReceipt(txHash: string): Promise<any> {
    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline) {
        const receipt = await send("eth_getTransactionReceipt", [txHash]);
        if (receipt !== null && receipt !== undefined) {
            return receipt;
        }
        await sleep(POLL_INTERVAL_MS);
    }
    throw new Error(`transaction ${txHash} was not mined in time`);
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
