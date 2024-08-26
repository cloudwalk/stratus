import { expect } from "chai";
import { keccak256 } from "ethers";

import { ALICE, BOB } from "./helpers/account";
import { sendAndGetFullResponse, sendWithRetry, updateProviderUrl } from "./helpers/rpc";

describe("Leader & Follower importer integration test", function () {
    it("Validate initial states", async function () {
        // Validate initial Leader state and health
        updateProviderUrl("stratus");
        const leaderNode = await sendWithRetry("stratus_state", []);
        expect(leaderNode.is_leader).to.equal(true);
        expect(leaderNode.is_importer_shutdown).to.equal(true);
        const leaderHealth = await sendWithRetry("stratus_health", []);
        expect(leaderHealth).to.equal(true);

        // Validate initial Follower state and health
        updateProviderUrl("stratus-follower");
        const followerNode = await sendWithRetry("stratus_state", []);
        expect(followerNode.is_leader).to.equal(false);
        expect(followerNode.is_importer_shutdown).to.equal(false);
        const followerHealth = await sendWithRetry("stratus_health", []);
        expect(followerHealth).to.equal(true);
    });

    it("Shutdown importer", async function () {
        // Shutdown command to Leader should fail
        updateProviderUrl("stratus");
        const responseLeader = await sendAndGetFullResponse("stratus_shutdownImporter", []);
        expect(responseLeader.data.error.code).eq(-32009);
        expect(responseLeader.data.error.message).eq("Stratus node is not a follower.");

        // Send shutdown command to Follower
        updateProviderUrl("stratus-follower");
        const responseFollower = await sendAndGetFullResponse("stratus_shutdownImporter", []);
        expect(responseFollower.data.result).to.equal(true);

        // Validate Follower state and health
        const followerNode = await sendWithRetry("stratus_state", []);
        expect(followerNode.is_leader).to.equal(false);
        expect(followerNode.is_importer_shutdown).to.equal(true);
        const followerHealth = await sendAndGetFullResponse("stratus_health", []);
        expect(followerHealth.data.error.code).eq(-32009);
        expect(followerHealth.data.error.message).eq("Stratus is not ready to start servicing requests.");

        // Validate Leader state and health
        updateProviderUrl("stratus");
        const leaderNode = await sendWithRetry("stratus_state", []);
        expect(leaderNode.is_leader).to.equal(true);
        expect(leaderNode.is_importer_shutdown).to.equal(true);
        const leaderHealth = await sendAndGetFullResponse("stratus_health", []);
        expect(leaderHealth.data.result).to.equal(true);
    });

    it("(Importer Shutdown) Validate Follower is not importing blocks from Leader", async function () {
        // Validate Leader block is ahead of Follower
        const { leaderBlock, followerBlock } = await waitForLeaderToBeAhead();
        expect(parseInt(leaderBlock, 16)).to.be.greaterThan(parseInt(followerBlock, 16));
    });

    it("(Importer Shutdown) Validate Leader is able to receive Transactions", async function () {
        // Transactions on Leader should still succeed
        updateProviderUrl("stratus");
        const nonceResponse = await sendAndGetFullResponse("eth_getTransactionCount", [ALICE.address, "latest"]);
        const nonce = parseInt(nonceResponse.data.result, 16);
        const signedTx = await ALICE.signWeiTransfer(BOB.address, 1, nonce);
        const txHash = keccak256(signedTx);
        const txResponse = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(txResponse.data.result).to.equal(txHash);

        // Previous transaction on Leader should not appear on Follower
        updateProviderUrl("stratus-follower");
        const txResponseFollower = await getTransactionByHashUntilConfirmed(txHash);
        expect(txResponseFollower.data.result).to.equal(null);
    });

    it("(Importer Shutdown) Validate Follower is unable to receive Transactions", async function () {
        updateProviderUrl("stratus-follower");
        const nonceResponse = await sendAndGetFullResponse("eth_getTransactionCount", [BOB.address, "latest"]);
        const nonce = parseInt(nonceResponse.data.result, 16);
        const signedTx = await BOB.signWeiTransfer(ALICE.address, 1, nonce);
        const txResponse = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(txResponse.data.error.code).to.equal(-32603);
        expect(txResponse.data.error.message).to.equal("Consensus is temporarily unavailable for follower node.");
    });

    it("Restart importer and validate states", async function () {
        // Init command to Leader should fail
        updateProviderUrl("stratus");
        const responseLeader = await sendAndGetFullResponse("stratus_initImporter", [
            "http://0.0.0.0:3000/",
            "ws://0.0.0.0:3000/",
        ]);
        expect(responseLeader.data.error.code).to.equal(-32009);
        expect(responseLeader.data.error.message).to.equal("Stratus node is not a follower.");

        // Init command to Follower without addresses should fail
        updateProviderUrl("stratus-follower");
        const responseInvalidFollower = await sendAndGetFullResponse("stratus_initImporter", []);
        expect(responseInvalidFollower.data.error.code).to.equal(-32602);
        expect(responseInvalidFollower.data.error.message).to.equal("Expected String parameter, but received nothing.");

        // Init command to Follower with addresses should succeed
        const responseValidFollower = await sendAndGetFullResponse("stratus_initImporter", [
            "http://0.0.0.0:3000/",
            "ws://0.0.0.0:3000/",
        ]);
        expect(responseValidFollower.data.result).to.equal(true);

        // Init command to Follower when Importer is already running should fail
        const responseSecondInitFollower = await sendAndGetFullResponse("stratus_initImporter", [
            "http://0.0.0.0:3000/",
            "ws://0.0.0.0:3000/",
        ]);
        expect(responseSecondInitFollower.data.error.code).to.equal(-32603);
        expect(responseSecondInitFollower.data.error.message).to.equal("Importer is already running.");

        // Wait until Follower is in sync with Leader
        await waitForFollowerToSyncWithLeader();

        // Validate Follower state and health
        updateProviderUrl("stratus-follower");
        const followerNode = await sendWithRetry("stratus_state", []);
        expect(followerNode.is_leader).to.equal(false);
        expect(followerNode.is_importer_shutdown).to.equal(false);
        const followerHealth = await sendWithRetry("stratus_health", []);
        expect(followerHealth).to.equal(true);

        // Validate Leader state and health
        updateProviderUrl("stratus");
        const leaderNode = await sendWithRetry("stratus_state", []);
        expect(leaderNode.is_leader).to.equal(true);
        expect(leaderNode.is_importer_shutdown).to.equal(true);
        const leaderHealth = await sendWithRetry("stratus_health", []);
        expect(leaderHealth).to.equal(true);
    });

    it("(Importer Restarted) Validate Leader is able to receive Transactions", async function () {
        // Transactions on Leader should succeed
        updateProviderUrl("stratus");
        const nonceResponse = await sendAndGetFullResponse("eth_getTransactionCount", [ALICE.address, "latest"]);
        const nonce = parseInt(nonceResponse.data.result, 16);
        const signedTx = await ALICE.signWeiTransfer(BOB.address, 1, nonce);
        const txHash = keccak256(signedTx);
        const txResponse = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(txResponse.data.result).to.equal(txHash);

        // Previous transaction on Leader should appear on Follower
        updateProviderUrl("stratus-follower");
        const txResponseFollower = await getTransactionByHashUntilConfirmed(txHash);
        expect(txResponseFollower.data.result.hash).to.equal(txHash);
    });

    it("(Importer Restarted) Validate Follower is able to receive Transactions", async function () {
        // Transactions on Follower should succeed
        updateProviderUrl("stratus-follower");
        const nonceResponse = await sendAndGetFullResponse("eth_getTransactionCount", [BOB.address, "latest"]);
        const nonce = parseInt(nonceResponse.data.result, 16);
        const signedTx = await BOB.signWeiTransfer(ALICE.address, 1, nonce);
        const txHash = keccak256(signedTx);
        const txResponse = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(txResponse.data.result).to.equal(txHash);

        // Previous transaction on Follower should forward to Leader
        updateProviderUrl("stratus");
        const txResponseLeader = await getTransactionByHashUntilConfirmed(txHash);
        expect(txResponseLeader.data.result.hash).to.equal(txHash);
    });
});

async function waitForLeaderToBeAhead() {
    const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
    const blocksAhead = 10;

    while (true) {
        updateProviderUrl("stratus");
        const leaderBlock = await sendWithRetry("eth_blockNumber", []);

        updateProviderUrl("stratus-follower");
        const followerBlock = await sendWithRetry("eth_blockNumber", []);

        if (parseInt(leaderBlock, 16) > parseInt(followerBlock, 16) + blocksAhead) {
            return { leaderBlock, followerBlock };
        }

        await delay(1000);
    }
}

async function waitForFollowerToSyncWithLeader() {
    const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

    while (true) {
        updateProviderUrl("stratus");
        const leaderBlock = await sendWithRetry("eth_blockNumber", []);

        updateProviderUrl("stratus-follower");
        const followerBlock = await sendWithRetry("eth_blockNumber", []);

        if (parseInt(leaderBlock, 16) === parseInt(followerBlock, 16)) {
            return { leaderBlock, followerBlock };
        }

        await delay(1000);
    }
}

async function getTransactionByHashUntilConfirmed(txHash: string, maxRetries: number = 3) {
    const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
    let txResponse = null;
    let retries = 0;

    while (retries < maxRetries) {
        txResponse = await sendAndGetFullResponse("eth_getTransactionByHash", [txHash]);

        if (txResponse.data.result) {
            return txResponse;
        }

        retries++;
        await delay(1000);
    }

    return txResponse;
}
