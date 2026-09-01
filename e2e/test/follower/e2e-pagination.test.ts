import { expect } from "chai";

import { ALICE } from "../helpers/account";
import { CHAIN_ID_DEC, send, sendAndGetFullResponse } from "../helpers/rpc";
import { FOLLOWER_URL, rpcCall, waitForFollowerBlock, waitForReceipt } from "./helpers";

// Requires the `just e2e-leader-follower-pagination` recipe: leader and follower both running
// with MAX_RESPONSE_SIZE_BYTES=8192. The rule under test is simple — when the serialized response
// exceeds the limit it must be paginated, and the follower must still sync the block.

const MAX_RESPONSE_BYTES = 8192;
const FAT_TX_DATA_BYTES = 50_000;

describe("Pagination", () => {
    it("paginates oversized importer responses and keeps the follower syncing", async () => {
        // a fitting response is served normally, with no envelope, so old followers are unaffected
        const earlyBlock = await send("eth_getBlockByNumber", ["0x1", false]);
        expect(earlyBlock).to.not.be.null;
        const small = await send("stratus_getBlockAndReceipts", [earlyBlock.hash]);
        expect(small.__stratus_paginated__).to.be.undefined;
        expect(small.block.number).to.equal("0x1");

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
        let assembled = "";
        let total = 0;
        for (let offset = 0; total === 0 || assembled.length < total; offset = assembled.length) {
            const envelope = await send("stratus_getBlockAndReceipts", [fatBlockHash, { offset: offset }]);
            expect(envelope.__stratus_paginated__).to.not.be.undefined;
            total = envelope.__stratus_paginated__.total;
            expect(envelope.__stratus_paginated__.chunk.length).to.be.greaterThan(0);
            assembled += envelope.__stratus_paginated__.chunk;
        }
        expect(assembled.length).to.equal(total);
        expect(total).to.be.greaterThan(MAX_RESPONSE_BYTES, "the block response should be oversized");

        // the reassembled content matches the block
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
