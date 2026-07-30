import { expect } from "chai";
import { keccak256 } from "ethers";

import { ALICE, BOB } from "./helpers/account";
import { getTransactionByHashUntilConfirmed, sendAndGetFullResponse, sendWithRetry, updateProviderUrl } from "./helpers/rpc";

// Leader & Fake Leader integration test.
//
// A fake leader imports blocks from the leader like a follower, but re-executes each block's
// transactions locally like a leader. After mining, `FakeLeaderWorker::import` compares the
// locally-mined `changes` and `block` against the leader's replicated `ExecutionChanges` and
// `Block`. Only if both match does it commit the block.
//
// This test asserts the desired end state: a transaction sent to the leader must be re-executed
// and replicated onto the fake leader.
describe("Leader & Fake Leader integration test", function () {
    it("Validate Leader state and health", async function () {
        updateProviderUrl("stratus");
        const leaderNode = await sendWithRetry("stratus_state", []);
        expect(leaderNode.is_leader).to.equal(true);
        const leaderHealth = await sendWithRetry("stratus_health", []);
        expect(leaderHealth).to.equal(true);
    });

    it("Validate Fake Leader state and health", async function () {
        updateProviderUrl("stratus-fake-leader");
        const fakeLeaderNode = await sendWithRetry("stratus_state", []);
        // A fake leader is leader-like: it accepts transactions and mines locally, but it also
        // runs the importer (so `is_importer_shutdown` is false).
        expect(fakeLeaderNode.is_leader).to.equal(true);
        expect(fakeLeaderNode.is_importer_shutdown).to.equal(false);
        const fakeLeaderHealth = await sendWithRetry("stratus_health", []);
        expect(fakeLeaderHealth).to.equal(true);
    });

    it("Fake Leader syncs empty blocks from Leader before any transaction", async function () {
        // Empty blocks have matching (empty) changes, so the consistency check passes and the
        // fake leader should stay in sync with the leader until a transaction is sent.
        await waitForFakeLeaderToSyncWithLeader();
    });

    let txHash: string;
    it("Send transaction to Leader", async function () {
        updateProviderUrl("stratus");
        const nonceHex = await sendWithRetry("eth_getTransactionCount", [ALICE.address, "latest"]);
        const nonce = parseInt(nonceHex, 16);
        const signedTx = await ALICE.signWeiTransfer(BOB.address, 1, nonce);
        txHash = keccak256(signedTx);
        const txResponse = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(txResponse.data.result).to.equal(txHash);
    });

    it("Fake Leader re-executes and replicates the transaction from Leader", async function () {
        // The fake leader imports the leader's block, re-executes the transaction locally, mines a
        // block and compares it against the leader's replicated data. If the check passes the
        // block is committed and the transaction becomes visible here.
        updateProviderUrl("stratus-fake-leader");
        const confirmed = await waitForTransactionOnFakeLeader(txHash, 30);
        expect(confirmed, "transaction was not replicated to the fake leader").to.equal(true);
    });

    it("Fake Leader block number catches up to Leader after transaction", async function () {
        await waitForFakeLeaderToSyncWithLeader();
    });
});

/// Polls until the fake leader's block number equals the leader's, failing after `maxAttempts`.
async function waitForFakeLeaderToSyncWithLeader(maxAttempts = 30) {
    const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
    for (let attempt = 0; attempt < maxAttempts; attempt++) {
        updateProviderUrl("stratus");
        const leaderBlock = await sendWithRetry("eth_blockNumber", []);
        updateProviderUrl("stratus-fake-leader");
        const fakeLeaderBlock = await sendWithRetry("eth_blockNumber", []);
        if (parseInt(leaderBlock, 16) === parseInt(fakeLeaderBlock, 16)) {
            return;
        }
        await delay(1000);
    }
    // Reuse the existing confirmation helper so the failure surfaces a clear message.
    throw new Error("fake leader did not sync with leader within the expected time");
}

/// Polls for a transaction on the fake leader, returning true once it appears.
async function waitForTransactionOnFakeLeader(txHash: string, maxAttempts = 30) {
    const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
    for (let attempt = 0; attempt < maxAttempts; attempt++) {
        updateProviderUrl("stratus-fake-leader");
        const response = await getTransactionByHashUntilConfirmed(txHash, 1);
        if (response.data.result) {
            return true;
        }
        await delay(1000);
    }
    return false;
}
