import axios from "axios";
import { expect } from "chai";

import { ALICE } from "../helpers/account";
import { CHAIN_ID_DEC, send, sendAndGetFullResponse } from "../helpers/rpc";

// This test only makes sense with a leader and a follower running with small response size
// limits AND the follower in block-changes replication mode (ENABLE_BLOCK_CHANGES_REPLICATION=true),
// so the importer syncs through `stratus_getBlockWithChanges` instead of block + receipts.
// It is executed by the `just e2e-leader-follower-pagination-changes` recipe.
//
// What it covers, in order:
// 1. Confirms a small block-with-changes response is served normally, with no pagination envelope.
// 2. Mines a fat block whose serialized `stratus_getBlockWithChanges` response exceeds the limits.
// 3. Confirms the old single-parameter call fails with the oversized response error (-32008).
// 4. Reassembles the with-changes response from the paginated envelope chunks and validates content.
// 5. Waits for the follower to sync the fat block and checks its content matches the leader.

// Fat transaction payload: enough data for the block + changes response to exceed the limits.
const FAT_TX_DATA_BYTES = 50_000;

// Small chunk budget to force several round trips during reassembly.
const CHUNK_BUDGET = 1024;

const FOLLOWER_URL = process.env.FOLLOWER_URL || "http://localhost:3001";
const LEADER_MAX_RESPONSE_BYTES = parseInt(process.env.MAX_RESPONSE_SIZE_BYTES || "8192");
const FOLLOWER_SYNC_TIMEOUT_MS = 120_000;
const POLL_INTERVAL_MS = 500;

describe("Pagination (block changes replication)", () => {
    it("paginates oversized stratus_getBlockWithChanges responses and keeps the follower syncing", async () => {
        // compatibility: a small early block is served normally, with no pagination envelope,
        // so an old follower keeps receiving the exact same responses as before;
        // the RocksDB-flavored block serializes hashes as byte arrays and numbers in
        // big-endian form, so content is asserted through the hash
        const earlyBlock = await send("eth_getBlockByNumber", ["0x1", false]);
        expect(earlyBlock).to.not.be.null;
        const small = await send("stratus_getBlockWithChanges", [earlyBlock.hash]);
        expect(small.__stratus_paginated__).to.be.undefined;
        expect(Buffer.from(small[0].header.hash).toString("hex")).to.equal(earlyBlock.hash.slice(2));

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
        const fatBlock = await send("eth_getBlockByNumber", [toHexNumber(fatBlockNumber), false]);
        const fatBlockHash = fatBlock.hash;

        // old-style single-parameter call: the oversized response is rejected by the server,
        // which is exactly what would get an old importer stuck retrying forever
        const legacy = await sendAndGetFullResponse("stratus_getBlockWithChanges", [fatBlockHash]);
        expect(legacy.data.error).to.not.be.undefined;
        expect(legacy.data.error.code).to.equal(-32008);

        // paginated reassembly
        let assembled = "";
        let total = 0;
        for (let offset = 0; total === 0 || assembled.length < total; offset = assembled.length) {
            const envelope = await send("stratus_getBlockWithChanges", [
                fatBlockHash,
                { offset: offset, chunk_budget: CHUNK_BUDGET },
            ]);
            expect(envelope.__stratus_paginated__).to.not.be.undefined;
            total = envelope.__stratus_paginated__.total;
            expect(envelope.__stratus_paginated__.chunk.length).to.be.greaterThan(0);
            assembled += envelope.__stratus_paginated__.chunk;
        }
        expect(assembled.length).to.equal(total);
        expect(total).to.be.greaterThan(LEADER_MAX_RESPONSE_BYTES, "the with-changes response should be oversized");

        // the serialized form is the (block, changes) pair the follower importer deserializes;
        // the RocksDB-flavored block serializes hashes as byte arrays and numbers in
        // big-endian form, so content is asserted through the hash
        const parsed = JSON.parse(assembled);
        expect(parsed).to.be.an("array").with.length(2);
        expect(Buffer.from(parsed[0].header.hash).toString("hex")).to.equal(fatBlockHash.slice(2));
        expect(parsed[0].transactions).to.have.length(1);

        // the follower imports the fat block through the paginated with-changes importer
        await waitForFollowerBlock(fatBlockNumber);
        const followerBlock = await rpcCall(FOLLOWER_URL, "eth_getBlockByNumber", [toHexNumber(fatBlockNumber), true]);
        expect(followerBlock.result.hash).to.equal(fatBlockHash);
        expect(followerBlock.result.transactions).to.have.length(1);
        expect(followerBlock.result.transactions[0].hash).to.equal(txHash);
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
        if (receipt) {
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
