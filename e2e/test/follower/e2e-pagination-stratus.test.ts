import { expect } from "chai";

import { ALICE } from "../helpers/account";
import { CHAIN_ID_DEC, send } from "../helpers/rpc";
import { FOLLOWER_URL, rpcCall, waitForFollowerBlock, waitForReceipt } from "./helpers";

// Requires the `just e2e-leader-follower-pagination-stratus` recipe: leader and follower both
// running with MAX_RESPONSE_SIZE_BYTES=8192, and the follower importing through the
// stratus-native response format (`--response-format stratus`). The rule under test is that
// the follower re-executes and commits imported blocks identically to the alloy format.

const FAT_TX_DATA_BYTES = 50_000;

describe("Pagination (stratus-native importer format)", () => {
    it("syncs the follower through the stratus-native importer format", async () => {
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

        // the follower imports the fat block through the paginated stratus-format importer
        await waitForFollowerBlock(fatBlockNumber);

        // the follower re-executed and stored the block: it serves the same block content.
        // the leader block is requested thin: the leader's small response limit rejects the
        // full fat block over `eth_getBlockByNumber` (only importer methods paginate)
        const leaderBlock = await send("eth_getBlockByNumber", [receipt.blockNumber, false]);
        const followerBlock = await rpcCall(FOLLOWER_URL, "eth_getBlockByNumber", [receipt.blockNumber, true]);
        expect(followerBlock.result.hash).to.equal(leaderBlock.hash);
        expect(followerBlock.result.transactions).to.have.lengthOf(leaderBlock.transactions.length);
        expect(followerBlock.result.transactions[0].hash).to.equal(txHash);

        // and the transaction receipt is available on the follower
        const followerReceipt = await rpcCall(FOLLOWER_URL, "eth_getTransactionReceipt", [txHash]);
        expect(followerReceipt.result).to.not.be.null;
        expect(followerReceipt.result.blockNumber).to.equal(receipt.blockNumber);
    });
});
