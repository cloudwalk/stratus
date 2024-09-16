import { expect } from "chai";
import { keccak256 } from "ethers";

import { ALICE, BOB } from "./helpers/account";
import {
    getTransactionByHashUntilConfirmed,
    sendAndGetFullResponse,
    sendWithRetry,
    updateProviderUrl,
    waitForFollowerToSyncWithLeader,
    waitForLeaderToBeAhead,
} from "./helpers/rpc";

describe("Leader & Follower change integration test", function () {
    it("Validate initial Leader state and health", async function () {
        updateProviderUrl("stratus");
        const leaderNode = await sendWithRetry("stratus_state", []);
        expect(leaderNode.is_importer_shutdown).to.equal(true);
        expect(leaderNode.is_interval_miner_running).to.equal(true);
        expect(leaderNode.is_leader).to.equal(true);
        expect(leaderNode.miner_paused).to.equal(false);
        expect(leaderNode.transactions_enabled).to.equal(true);
        const leaderHealth = await sendWithRetry("stratus_health", []);
        expect(leaderHealth).to.equal(true);
    });

    it("Validate initial Follower state and health", async function () {
        updateProviderUrl("stratus-follower");
        const followerNode = await sendWithRetry("stratus_state", []);
        expect(followerNode.is_importer_shutdown).to.equal(false);
        expect(followerNode.is_interval_miner_running).to.equal(false);
        expect(followerNode.is_leader).to.equal(false);
        expect(followerNode.miner_paused).to.equal(false);
        expect(followerNode.transactions_enabled).to.equal(true);
        const followerHealth = await sendWithRetry("stratus_health", []);
        expect(followerHealth).to.equal(true);
    });

    it("Change Leader to Leader should return false", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_changeToLeader", []);
        expect(response.data.result).to.equal(false);
    });

    it("Change Leader to Follower with transactions enabled should fail", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_changeToFollower", [
            "http://0.0.0.0:3001/",
            "ws://0.0.0.0:3001/",
            "2s",
            "100ms",
        ]);
        expect(response.data.error.code).to.equal(-32009);
        expect(response.data.error.message).to.equal("Transaction processing is enabled.");
    });

    it("Change Leader to Follower with miner enabled should fail ", async function () {
        updateProviderUrl("stratus");
        await sendWithRetry("stratus_disableTransactions", []);
        const response = await sendAndGetFullResponse("stratus_changeToFollower", [
            "http://0.0.0.0:3001/",
            "ws://0.0.0.0:3001/",
            "2s",
            "100ms",
        ]);
        expect(response.data.error.code).to.equal(-32603);
        expect(response.data.error.message).to.equal("Miner is enabled.");
    });

    it("Change Leader to Follower should succeed", async function () {
        updateProviderUrl("stratus");
        await sendWithRetry("stratus_disableMiner", []);
        await new Promise((resolve) => setTimeout(resolve, 4000));
        const response = await sendAndGetFullResponse("stratus_changeToFollower", [
            "http://0.0.0.0:3001/",
            "ws://0.0.0.0:3001/",
            "2s",
            "100ms",
        ]);
        expect(response.data.result).to.equal(true);
    });

    it("Validate new Follower health and state after change", async function () {
        updateProviderUrl("stratus");
        await new Promise((resolve) => setTimeout(resolve, 4000));
        const response = await sendWithRetry("stratus_health", []);
        const followerNode = await sendWithRetry("stratus_state", []);
        expect(followerNode.is_importer_shutdown).to.equal(false);
        expect(followerNode.is_interval_miner_running).to.equal(false);
        expect(followerNode.is_leader).to.equal(false);
        expect(followerNode.miner_paused).to.equal(false);
        expect(followerNode.transactions_enabled).to.equal(false);
        expect(response).to.equal(true);
    });

    it("Change Follower to Follower should fail", async function () {
        updateProviderUrl("stratus-follower");
        const response = await sendAndGetFullResponse("stratus_changeToFollower", []);
        expect(response.data.result).to.equal(false);
    });

    it("Change Follower to Leader with transactions enabled should fail", async function () {
        updateProviderUrl("stratus-follower");
        const response = await sendAndGetFullResponse("stratus_changeToLeader", []);
        expect(response.data.error.code).to.equal(-32009);
        expect(response.data.error.message).to.equal("Transaction processing is enabled.");
    });

    it("Change Follower to Leader should succeed", async function () {
        updateProviderUrl("stratus-follower");
        await sendWithRetry("stratus_disableTransactions", []);
        const response = await sendAndGetFullResponse("stratus_changeToLeader", []);
        expect(response.data.result).to.equal(true);
    });

    it("Validate new Leader health and state after change", async function () {
        updateProviderUrl("stratus-follower");
        await new Promise((resolve) => setTimeout(resolve, 4000));
        const response = await sendWithRetry("stratus_health", []);
        expect(response).to.equal(true);
        const leaderNode = await sendWithRetry("stratus_state", []);
        expect(leaderNode.is_importer_shutdown).to.equal(true);
        expect(leaderNode.is_interval_miner_running).to.equal(true);
        expect(leaderNode.is_leader).to.equal(true);
        expect(leaderNode.miner_paused).to.equal(false);
        expect(leaderNode.transactions_enabled).to.equal(false);
    });

    it("Change new Leader to Follower again should succeed", async function () {
        updateProviderUrl("stratus-follower");
        await sendWithRetry("stratus_disableTransactions", []);
        await sendWithRetry("stratus_disableMiner", []);
        await new Promise((resolve) => setTimeout(resolve, 4000));
        const response = await sendAndGetFullResponse("stratus_changeToFollower", [
            "http://0.0.0.0:3000/",
            "ws://0.0.0.0:3000/",
            "2s",
            "100ms",
        ]);
        expect(response.data.result).to.equal(true);
    });

    it("Validate new Follower health and state after change", async function () {
        updateProviderUrl("stratus-follower");
        await new Promise((resolve) => setTimeout(resolve, 4000));
        const response = await sendWithRetry("stratus_health", []);
        expect(response).to.equal(true);
        const followerNode = await sendWithRetry("stratus_state", []);
        expect(followerNode.is_importer_shutdown).to.equal(false);
        expect(followerNode.is_interval_miner_running).to.equal(false);
        expect(followerNode.is_leader).to.equal(false);
        expect(followerNode.miner_paused).to.equal(false);
        expect(followerNode.transactions_enabled).to.equal(false);
    });

    it("Change new Follower to Leader again should succeed", async function () {
        updateProviderUrl("stratus");
        await sendWithRetry("stratus_disableTransactions", []);
        await new Promise((resolve) => setTimeout(resolve, 4000));
        const response = await sendAndGetFullResponse("stratus_changeToLeader", []);
        expect(response.data.result).to.equal(true);
    });

    it("Validate new Leader health and state after change", async function () {
        updateProviderUrl("stratus");
        await new Promise((resolve) => setTimeout(resolve, 4000));
        const response = await sendWithRetry("stratus_health", []);
        expect(response).to.equal(true);
        const leaderNode = await sendWithRetry("stratus_state", []);
        expect(leaderNode.is_importer_shutdown).to.equal(true);
        //expect(leaderNode.is_interval_miner_running).to.equal(true);
        expect(leaderNode.is_leader).to.equal(true);
        expect(leaderNode.miner_paused).to.equal(false);
        expect(leaderNode.transactions_enabled).to.equal(false);
    });
});
