import { expect } from "chai";

import { ALICE } from "../helpers/account";
import { CHAIN_ID_DEC, send, sendAndGetFullResponse } from "../helpers/rpc";
import { FOLLOWER_URL, rpcCall, waitForFollowerBlock, waitForReceipt } from "./helpers";

// Requires the `just e2e-leader-follower-pagination` recipe: leader and follower both running
// with MAX_RESPONSE_SIZE_BYTES=8192. The rule under test is simple — when the serialized response
// exceeds the limit it must be paginated, and the follower must still sync the block.

const MAX_RESPONSE_BYTES = 8192;
const FAT_TX_DATA_BYTES = 50_000;

// The stratus-native block DTO serializes hashes as byte arrays; convert them to hex for assertions.
const bytesToHex = (bytes: number[]) => "0x" + Buffer.from(bytes).toString("hex");

// The block DTO serializes block numbers as byte-order-swapped u32 (paired to_be/from_be serde in
// Rust, the same wire contract as `stratus_get_block_with_changes`); swap them back for assertions.
const swapU32 = (value: number) =>
    ((value & 0xff) << 24) | ((value & 0xff00) << 8) | ((value >>> 8) & 0xff00) | (value >>> 24);

describe("Pagination", () => {
    it("paginates oversized importer responses and keeps the follower syncing", async () => {
        // a fitting response is served normally, with no envelope, so old followers are unaffected
        const earlyBlock = await send("eth_getBlockByNumber", ["0x1", false]);
        expect(earlyBlock).to.not.be.null;
        const small = await send("stratus_getBlockAndReceipts", [earlyBlock.hash]);
        expect(small.stratus_paginated).to.be.undefined;
        expect(small.block.number).to.equal("0x1");

        // the stratus-native format serves the block DTO directly, still without envelope when it fits
        const smallStratus = await send("stratus_getBlockAndReceipts", [
            earlyBlock.hash,
            { offset: 0, format: "stratus" },
        ]);
        expect(smallStratus.stratus_paginated).to.be.undefined;
        expect(smallStratus.block).to.be.undefined;
        expect(smallStratus.receipts).to.be.undefined;
        expect(bytesToHex(smallStratus.header.hash)).to.equal(earlyBlock.hash);
        expect(swapU32(smallStratus.header.number)).to.equal(1);
        expect(smallStratus.transactions).to.be.an("array");

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
        expect(legacy.data.error).to.not.be.undefined;
        expect(legacy.data.error.code).to.equal(-32008);

        // paginated reassembly; the chunk size is decided by the leader's response size limit
        let assembled: Buffer = Buffer.alloc(0);
        let total = 0;
        for (let offset = 0; total === 0 || assembled.length < total; offset = assembled.length) {
            const envelope = await send("stratus_getBlockAndReceipts", [fatBlockHash, { offset: offset }]);
            expect(envelope.stratus_paginated).to.not.be.undefined;
            total = envelope.stratus_paginated.total;
            const chunk = Buffer.from(envelope.stratus_paginated.chunk, "base64");
            expect(chunk.length).to.be.greaterThan(0);
            assembled = Buffer.concat([assembled, chunk]);
        }
        expect(assembled.length).to.equal(total);
        expect(total).to.be.greaterThan(MAX_RESPONSE_BYTES, "the block response should be oversized");

        // the reassembled content matches the block
        const response = JSON.parse(assembled.toString("utf8"));
        expect(response.block.hash).to.equal(fatBlockHash);
        expect(parseInt(response.block.number, 16)).to.equal(fatBlockNumber);
        expect(response.block.transactions).to.have.length(1);
        expect(response.receipts).to.have.length(1);
        expect(response.receipts[0].transactionHash).to.equal(txHash);

        // the same oversized block paginates identically in the stratus-native format,
        // with the format field riding every chunk request
        let stratusAssembled: Buffer = Buffer.alloc(0);
        let stratusTotal = 0;
        for (
            let offset = 0;
            stratusTotal === 0 || stratusAssembled.length < stratusTotal;
            offset = stratusAssembled.length
        ) {
            const envelope = await send("stratus_getBlockAndReceipts", [
                fatBlockHash,
                { offset: offset, format: "stratus" },
            ]);
            expect(envelope.stratus_paginated).to.not.be.undefined;
            stratusTotal = envelope.stratus_paginated.total;
            const chunk = Buffer.from(envelope.stratus_paginated.chunk, "base64");
            expect(chunk.length).to.be.greaterThan(0);
            stratusAssembled = Buffer.concat([stratusAssembled, chunk]);
        }
        expect(stratusAssembled.length).to.equal(stratusTotal);
        expect(stratusTotal).to.be.greaterThan(MAX_RESPONSE_BYTES, "the stratus response should be oversized");

        // the reassembled stratus content has the block DTO shape, with receipts embedded
        const stratusResponse = JSON.parse(stratusAssembled.toString("utf8"));
        expect(stratusResponse.block).to.be.undefined;
        expect(stratusResponse.receipts).to.be.undefined;
        expect(bytesToHex(stratusResponse.header.hash)).to.equal(fatBlockHash);
        expect(swapU32(stratusResponse.header.number)).to.equal(fatBlockNumber);
        expect(stratusResponse.transactions).to.have.length(1);
        expect(bytesToHex(stratusResponse.transactions[0].input.hash)).to.equal(txHash);
        expect(stratusResponse.transactions[0].execution).to.not.be.undefined;
        expect(stratusResponse.transactions[0].logs).to.not.be.undefined;

        // the follower imports the fat block through the paginated importer
        await waitForFollowerBlock(fatBlockNumber);
        const followerReceipt = await rpcCall(FOLLOWER_URL, "eth_getTransactionReceipt", [txHash]);
        expect(followerReceipt.result).to.not.be.null;
        expect(followerReceipt.result.blockNumber).to.equal(receipt.blockNumber);
    });
});
