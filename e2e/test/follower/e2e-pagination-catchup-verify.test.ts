import { expect } from "chai";
import fs from "fs";

import { FOLLOWER_URL, LEADER_URL, rpcCall, toHexNumber, waitForFollowerBlock } from "./helpers";

// Part 2 of the catch-up scenario: the follower started several fat blocks behind and must
// catch up through all the paginated responses, serving every fat block correctly.

const ARTIFACT_FILE = "pagination-catchup.json";

describe("Pagination catch-up (verify)", () => {
    it("catches up through multiple paginated fat blocks", async () => {
        const artifact = JSON.parse(fs.readFileSync(ARTIFACT_FILE, "utf-8"));
        expect(artifact.blocks.length).to.be.greaterThan(1, "the setup step must have mined several fat blocks");

        // the follower was behind and must reach the fat blocks mined while it was down
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
