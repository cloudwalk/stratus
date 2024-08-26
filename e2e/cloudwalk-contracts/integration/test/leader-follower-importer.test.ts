import { expect } from "chai";
import { keccak256 } from "ethers";

import { ALICE, BOB } from "./helpers/account";
import { sendAndGetFullResponse, sendWithRetry, updateProviderUrl } from "./helpers/rpc";

describe("Leader & Follower importer integration test", function () {
    let txHash: string;

    it("Validate initial Leader state and health", async function () {
        updateProviderUrl("stratus");
        const leaderNode = await sendWithRetry("stratus_state", []);
        expect(leaderNode.is_leader).to.equal(true);
        expect(leaderNode.is_importer_shutdown).to.equal(true);
        const leaderHealth = await sendWithRetry("stratus_health", []);
        expect(leaderHealth).to.equal(true);
    });

    it("Validate initial Follower state and health", async function () {
        updateProviderUrl("stratus-follower");
        const followerNode = await sendWithRetry("stratus_state", []);
        expect(followerNode.is_leader).to.equal(false);
        expect(followerNode.is_importer_shutdown).to.equal(false);
        const followerHealth = await sendWithRetry("stratus_health", []);
        expect(followerHealth).to.equal(true);
    });

    it("Shutdown command to Leader should fail", async function () {
        updateProviderUrl("stratus");
        const responseLeader = await sendAndGetFullResponse("stratus_shutdownImporter", []);
        expect(responseLeader.data.error.code).eq(-32009);
        expect(responseLeader.data.error.message).eq("Stratus node is not a follower.");
    });

    it("Shutdown command to Follower should succeed", async function () {
        updateProviderUrl("stratus-follower");
        const responseFollower = await sendAndGetFullResponse("stratus_shutdownImporter", []);
        expect(responseFollower.data.result).to.equal(true);
    });

    it("Validate Follower state and health after shutdown", async function () {
        updateProviderUrl("stratus-follower");
        const followerNode = await sendWithRetry("stratus_state", []);
        expect(followerNode.is_leader).to.equal(false);
        expect(followerNode.is_importer_shutdown).to.equal(true);
        const followerHealth = await sendAndGetFullResponse("stratus_health", []);
        expect(followerHealth.data.error.code).eq(-32009);
        expect(followerHealth.data.error.message).eq("Stratus is not ready to start servicing requests.");
    });

    it("Validate Leader state and health after Follower shutdown", async function () {
        updateProviderUrl("stratus");
        const leaderNode = await sendWithRetry("stratus_state", []);
        expect(leaderNode.is_leader).to.equal(true);
        expect(leaderNode.is_importer_shutdown).to.equal(true);
        const leaderHealth = await sendAndGetFullResponse("stratus_health", []);
        expect(leaderHealth.data.result).to.equal(true);
    });

    it("Validate Leader block is ahead of Follower after Importer shutdown", async function () {
        const { leaderBlock, followerBlock } = await waitForLeaderToBeAhead();
        expect(parseInt(leaderBlock, 16)).to.be.greaterThan(parseInt(followerBlock, 16));
    });

    it("Transactions on Leader should still succeed after Importer shutdown", async function () {
        updateProviderUrl("stratus");
        const nonceResponse = await sendAndGetFullResponse("eth_getTransactionCount", [ALICE.address, "latest"]);
        const nonce = parseInt(nonceResponse.data.result, 16);
        const signedTx = await ALICE.signWeiTransfer(BOB.address, 1, nonce);
        txHash = keccak256(signedTx);
        const txResponse = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(txResponse.data.result).to.equal(txHash);
    });

    it("Previous transaction on Leader should not appear on Follower when Importer is shutdown", async function () {
        updateProviderUrl("stratus-follower");
        const txResponseFollower = await getTransactionByHashUntilConfirmed(txHash);
        expect(txResponseFollower.data.result).to.equal(null);
        txHash = "";
    });

    it("Transactions on Follower should fail when Importer is shutdown", async function () {
        updateProviderUrl("stratus-follower");
        const nonceResponse = await sendAndGetFullResponse("eth_getTransactionCount", [BOB.address, "latest"]);
        const nonce = parseInt(nonceResponse.data.result, 16);
        const signedTx = await BOB.signWeiTransfer(ALICE.address, 1, nonce);
        const txResponse = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(txResponse.data.error.code).to.equal(-32603);
        expect(txResponse.data.error.message).to.equal("Consensus is temporarily unavailable for follower node.");
    });

    it("Init command to Leader should fail", async function () {
        updateProviderUrl("stratus");
        const responseLeader = await sendAndGetFullResponse("stratus_initImporter", [
            "http://0.0.0.0:3000/",
            "ws://0.0.0.0:3000/",
        ]);
        expect(responseLeader.data.error.code).to.equal(-32009);
        expect(responseLeader.data.error.message).to.equal("Stratus node is not a follower.");
    });

    it("Init command to Follower without addresses should fail", async function () {
        updateProviderUrl("stratus-follower");
        const responseInvalidFollower = await sendAndGetFullResponse("stratus_initImporter", []);
        expect(responseInvalidFollower.data.error.code).to.equal(-32602);
        expect(responseInvalidFollower.data.error.message).to.equal("Expected String parameter, but received nothing.");
    });

    it("Init command to Follower with invalid addresses should fail", async function () {
        const responseInvalidAddressFollower = await sendAndGetFullResponse("stratus_initImporter", [
            "http://0.0.0.0:9999/",
            "ws://0.0.0.0:9999/",
        ]);
        expect(responseInvalidAddressFollower.data.error.code).to.equal(-32603);
        expect(responseInvalidAddressFollower.data.error.message).to.equal("Failed to initialize importer.");
    });

    it("Init command to Follower with valid addresses should succeed", async function () {
        const responseValidFollower = await sendAndGetFullResponse("stratus_initImporter", [
            "http://0.0.0.0:3000/",
            "ws://0.0.0.0:3000/",
        ]);
        expect(responseValidFollower.data.result).to.equal(true);
    });

    it("Init command to Follower when Importer is already running should fail", async function () {
        const responseSecondInitFollower = await sendAndGetFullResponse("stratus_initImporter", [
            "http://0.0.0.0:3000/",
            "ws://0.0.0.0:3000/",
        ]);
        expect(responseSecondInitFollower.data.error.code).to.equal(-32603);
        expect(responseSecondInitFollower.data.error.message).to.equal("Importer is already running.");
    });

    it("Wait until Follower is in sync with Leader", async function () {
        await waitForFollowerToSyncWithLeader();
    });

    it("Validate Follower state and health after Importer restart", async function () {
        updateProviderUrl("stratus-follower");
        const followerNode = await sendWithRetry("stratus_state", []);
        expect(followerNode.is_leader).to.equal(false);
        expect(followerNode.is_importer_shutdown).to.equal(false);
        const followerHealth = await sendWithRetry("stratus_health", []);
        expect(followerHealth).to.equal(true);
    });

    it("Validate Leader state and health after Importer restart", async function () {
        updateProviderUrl("stratus");
        const leaderNode = await sendWithRetry("stratus_state", []);
        expect(leaderNode.is_leader).to.equal(true);
        expect(leaderNode.is_importer_shutdown).to.equal(true);
        const leaderHealth = await sendWithRetry("stratus_health", []);
        expect(leaderHealth).to.equal(true);
    });

    it("Transactions on Leader should succeed after Importer restart", async function () {
        updateProviderUrl("stratus");
        const nonceResponse = await sendAndGetFullResponse("eth_getTransactionCount", [ALICE.address, "latest"]);
        const nonce = parseInt(nonceResponse.data.result, 16);
        const signedTx = await ALICE.signWeiTransfer(BOB.address, 1, nonce);
        txHash = keccak256(signedTx);
        const txResponse = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(txResponse.data.result).to.equal(txHash);
    });

    it("Previous transaction on Leader should appear on Follower when Importer is running", async function () {
        updateProviderUrl("stratus-follower");
        const txResponseFollower = await getTransactionByHashUntilConfirmed(txHash);
        expect(txResponseFollower.data.result.hash).to.equal(txHash);
        txHash = "";
    });

    it("Transactions on Follower should succeed and be forwarded to Leader after Importer restart", async function () {
        updateProviderUrl("stratus-follower");
        const nonceResponse = await sendAndGetFullResponse("eth_getTransactionCount", [BOB.address, "latest"]);
        const nonce = parseInt(nonceResponse.data.result, 16);
        const signedTx = await BOB.signWeiTransfer(ALICE.address, 1, nonce);
        txHash = keccak256(signedTx);
        const txResponse = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(txResponse.data.result).to.equal(txHash);
    });

    it("Previous transaction on Follower should forward and exist on Leader", async function () {
        updateProviderUrl("stratus");
        const txResponseLeader = await getTransactionByHashUntilConfirmed(txHash);
        expect(txResponseLeader.data.result.hash).to.equal(txHash);
        txHash = "";
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
