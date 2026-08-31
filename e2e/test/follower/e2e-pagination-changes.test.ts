import { expect } from "chai";

import { ALICE } from "../helpers/account";
import { CHAIN_ID_DEC, send, sendAndGetFullResponse } from "../helpers/rpc";
import { FOLLOWER_URL, rpcCall, toHexNumber, waitForFollowerBlock, waitForReceipt } from "./helpers";

// Requires the `just e2e-leader-follower-pagination-changes` recipe: a follower in block-changes
// replication mode, with both response limits small. Same rule as e2e-pagination.test.ts, through
// the stratus_getBlockWithChanges endpoint instead.

const MAX_RESPONSE_BYTES = 8192;
const FAT_TX_DATA_BYTES = 50_000;

describe("Pagination (block changes replication)", () => {
    it("paginates oversized stratus_getBlockWithChanges responses and keeps the follower syncing", async () => {
        // a fitting response is served normally, with no envelope, so old followers are unaffected;
        // the RocksDB-flavored block serializes hashes as byte arrays, so compare them as hex
        const earlyBlock = await send("eth_getBlockByNumber", ["0x1", false]);
        expect(earlyBlock).to.not.be.null;
        const small = await send("stratus_getBlockWithChanges", [earlyBlock.hash]);
        expect(small.__stratus_paginated__).to.be.undefined;
        expect(Buffer.from(small[0].header.hash).toString("hex")).to.equal(earlyBlock.hash.slice(2));

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

        // the old single-parameter call fails with the oversized response error (-32008)
        const legacy = await sendAndGetFullResponse("stratus_getBlockWithChanges", [fatBlockHash]);
        expect(legacy.data.error).to.not.be.undefined;
        expect(legacy.data.error.code).to.equal(-32008);

        // paginated reassembly, with a small chunk budget to force several round trips
        let assembled = "";
        let total = 0;
        for (let offset = 0; total === 0 || assembled.length < total; offset = assembled.length) {
            const envelope = await send("stratus_getBlockWithChanges", [
                fatBlockHash,
                { offset: offset, chunk_budget: 1024 },
            ]);
            expect(envelope.__stratus_paginated__).to.not.be.undefined;
            total = envelope.__stratus_paginated__.total;
            expect(envelope.__stratus_paginated__.chunk.length).to.be.greaterThan(0);
            assembled += envelope.__stratus_paginated__.chunk;
        }
        expect(assembled.length).to.equal(total);
        expect(total).to.be.greaterThan(MAX_RESPONSE_BYTES, "the with-changes response should be oversized");

        // the reassembled content is the (block, changes) pair the follower importer deserializes
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
