import { expect } from "chai";

import { ALICE } from "../helpers/account";
import { CHAIN_ID_DEC, send, sendAndGetFullResponse } from "../helpers/rpc";
import { FOLLOWER_URL, rpcCall, waitForFollowerBlock, waitForReceipt } from "./helpers";

// Requires the `just e2e-leader-follower-pagination` recipe: leader and follower both running
// with MAX_RESPONSE_SIZE_BYTES=8192. The rule under test is simple — when the serialized response
// exceeds the limit it must be paginated, and the follower must still sync the block.

const MAX_RESPONSE_BYTES = 8192;
const FAT_TX_DATA_BYTES = 50_000;

// The block DTO serializes hashes as byte arrays; convert them to hex for assertions.
const bytesToHex = (bytes: number[]) => "0x" + Buffer.from(bytes).toString("hex");

// The block DTO serializes block numbers as byte-order-swapped u32; swap them back for assertions.
const swapU32 = (value: number) =>
    ((value & 0xff) << 24) | ((value & 0xff00) << 8) | ((value >>> 8) & 0xff00) | (value >>> 24);

describe("Pagination", () => {
    it("paginates oversized importer responses and keeps the follower syncing", async () => {
        // a fitting response is served normally, with no envelope
        const earlyBlock = await send("eth_getBlockByNumber", ["0x1", false]);
        expect(earlyBlock).to.not.equal(null);
        const small = await send("stratus_getBlockAndReceipts", [earlyBlock.hash]);
        expect(small.stratus_paginated).to.equal(undefined);
        expect(small.block).to.equal(undefined);
        expect(small.receipts).to.equal(undefined);
        expect(bytesToHex(small.header.hash)).to.equal(earlyBlock.hash);
        expect(swapU32(small.header.number)).to.equal(1);
        expect(small.transactions).to.be.an("array");

        // fat contract deployment: the code always fails, but the fat data makes the response oversized
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
        const fatBlockHash = receipt.blockHash;

        // the old single-parameter call fails with the oversized response error (-32008),
        // which is exactly what would stall an importer before pagination existed
        const legacy = await sendAndGetFullResponse("stratus_getBlockAndReceipts", [fatBlockHash]);
        expect(legacy.data.error).to.not.equal(undefined);
        expect(legacy.data.error.code).to.equal(-32008);

        // paginated reassembly; the chunk size is decided by the leader's response size limit
        let assembled: Buffer = Buffer.alloc(0);
        let total = 0;
        for (let offset = 0; total === 0 || assembled.length < total; offset = assembled.length) {
            const envelope = await send("stratus_getBlockAndReceipts", [fatBlockHash, { offset: offset }]);
            expect(envelope.stratus_paginated).to.not.equal(undefined);
            total = envelope.stratus_paginated.total;
            const chunk = Buffer.from(envelope.stratus_paginated.chunk, "base64");
            expect(chunk.length).to.be.greaterThan(0);
            assembled = Buffer.concat([assembled, chunk]);
        }
        expect(assembled.length).to.equal(total);
        expect(total).to.be.greaterThan(MAX_RESPONSE_BYTES, "the block response should be oversized");

        // the reassembled content is the block DTO, with receipts embedded
        const response = JSON.parse(assembled.toString("utf8"));
        expect(response.block).to.equal(undefined);
        expect(response.receipts).to.equal(undefined);
        expect(bytesToHex(response.header.hash)).to.equal(fatBlockHash);
        expect(swapU32(response.header.number)).to.equal(fatBlockNumber);
        expect(response.transactions).to.have.length(1);
        expect(bytesToHex(response.transactions[0].input.hash)).to.equal(txHash);
        expect(response.transactions[0].execution).to.not.equal(undefined);
        expect(response.transactions[0].logs).to.not.equal(undefined);

        // the follower imports the fat block through the paginated importer
        await waitForFollowerBlock(fatBlockNumber);

        // the follower serves the same block content; the leader block is requested thin because
        // its response limit rejects the full fat block over `eth_getBlockByNumber`
        const leaderBlock = await send("eth_getBlockByNumber", [receipt.blockNumber, false]);
        const followerBlock = await rpcCall(FOLLOWER_URL, "eth_getBlockByNumber", [receipt.blockNumber, true]);
        expect(followerBlock.result.hash).to.equal(leaderBlock.hash);
        expect(followerBlock.result.transactions).to.have.lengthOf(leaderBlock.transactions.length);
        expect(followerBlock.result.transactions[0].hash).to.equal(txHash);

        // and the transaction receipt is available on the follower
        const followerReceipt = await rpcCall(FOLLOWER_URL, "eth_getTransactionReceipt", [txHash]);
        expect(followerReceipt.result).to.not.equal(null);
        expect(followerReceipt.result.blockNumber).to.equal(receipt.blockNumber);
    });
});
