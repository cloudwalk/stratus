import { expect } from "chai";
import { keccak256 } from "ethers";

import { ALICE, BOB } from "./helpers/account";
import {
    sendAndGetFullResponse,
    sendWithRetry,
    updateProviderUrl,
    waitForFollowerToSyncWithLeader,
} from "./helpers/rpc";

describe("Miner mode change integration test", function () {
    it("Validate initial Leader state and health", async function () {
        updateProviderUrl("stratus");
        const leaderNode = await sendWithRetry("stratus_state", []);
        expect(leaderNode.is_leader).to.equal(true);
        expect(leaderNode.miner_enabled).to.equal(true);
        expect(leaderNode.is_interval_miner_shutdown).to.equal(false);
        expect(leaderNode.transactions_enabled).to.equal(true);
        const leaderHealth = await sendWithRetry("stratus_health", []);
        expect(leaderHealth).to.equal(true);
    });

    it("Validate initial Follower state and health", async function () {
        updateProviderUrl("stratus-follower");
        const followerNode = await sendWithRetry("stratus_state", []);
        expect(followerNode.is_leader).to.equal(false);
        expect(followerNode.miner_enabled).to.equal(true);
        expect(followerNode.is_interval_miner_shutdown).to.equal(true);
        expect(followerNode.transactions_enabled).to.equal(true);
        const followerHealth = await sendWithRetry("stratus_health", []);
        expect(followerHealth).to.equal(true);
    });

    it("Miner change to External on Leader should fail because transactions are enabled", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_changeMinerMode", ["external"]);
        expect(response.data.error.code).eq(-32009);
        expect(response.data.error.message).eq("Transaction processing is enabled.");
    });

    it("Miner change to Interval on Follower should fail because transactions are enabled", async function () {
        updateProviderUrl("stratus-follower");
        const response = await sendAndGetFullResponse("stratus_changeMinerMode", ["1s"]);
        expect(response.data.error.code).eq(-32009);
        expect(response.data.error.message).eq("Transaction processing is enabled.");
    });

    it("Disable transactions on Leader", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_disableTransactions", []);
        expect(response.data.result).to.equal(false);
        const state = await sendWithRetry("stratus_state", []);
        expect(state.transactions_enabled).to.equal(false);
    });

    it("Disable transactions on Follower", async function () {
        updateProviderUrl("stratus-follower");
        const response = await sendAndGetFullResponse("stratus_disableTransactions", []);
        expect(response.data.result).to.equal(false);
        const state = await sendWithRetry("stratus_state", []);
        expect(state.transactions_enabled).to.equal(false);
    });

    it("Miner change to External on Leader should fail because miner is enabled", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_changeMinerMode", ["external"]);
        expect(response.data.error.code).eq(-32603);
        expect(response.data.error.message).eq("Miner is enabled.");
    });

    it("Miner change to Interval on Follower should fail because miner is enabled", async function () {
        updateProviderUrl("stratus-follower");
        const response = await sendAndGetFullResponse("stratus_changeMinerMode", ["1s"]);
        expect(response.data.error.code).eq(-32603);
        expect(response.data.error.message).eq("Miner is enabled.");
    });

    it("Disable miner on Leader", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_disableMiner", []);
        expect(response.data.result).to.equal(false);
        const state = await sendWithRetry("stratus_state", []);
        expect(state.miner_enabled).to.equal(false);
    });

    it("Disable miner on Follower", async function () {
        updateProviderUrl("stratus-follower");
        const response = await sendAndGetFullResponse("stratus_disableMiner", []);
        expect(response.data.result).to.equal(false);
        const state = await sendWithRetry("stratus_state", []);
        expect(state.miner_enabled).to.equal(false);
    });

    it("Miner change on Leader without params should fail", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_changeMinerMode", []);
        expect(response.data.error.code).eq(-32602);
        expect(response.data.error.message).eq("Expected String parameter, but received nothing.");
    });

    it("Miner change on Leader with invalid params should fail", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_changeMinerMode", ["invalidMode"]);
        expect(response.data.error.code).eq(-32603);
        expect(response.data.error.message).eq("Miner mode param is invalid.");
    });

    it("Miner change on Leader with same mode should return false", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_changeMinerMode", ["1s"]);
        expect(response.data.result).to.equal(false);
    });

    it("Miner change on Follower with same mode should return false", async function () {
        updateProviderUrl("stratus-follower");
        const response = await sendAndGetFullResponse("stratus_changeMinerMode", ["external"]);
        expect(response.data.result).to.equal(false);
    });

    it("Miner change on Leader to Automine should fail because it is not supported", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_changeMinerMode", ["automine"]);
        expect(response.data.error.code).eq(-32603);
        expect(response.data.error.message).eq("Miner mode change to (automine) is unsupported.");
    });

    it("Miner change on Leader to External should fail if there are pending transactions", async function () {
        // Enable Transactions back and disable Miner on Leader
        updateProviderUrl("stratus");
        const enableTransactionsResponse = await sendAndGetFullResponse("stratus_enableTransactions", []);
        expect(enableTransactionsResponse.data.result).to.equal(true);
        const disableMinerResponse = await sendAndGetFullResponse("stratus_disableMiner", []);
        expect(disableMinerResponse.data.result).to.equal(false);
        const leaderState = await sendWithRetry("stratus_state", []);
        expect(leaderState.transactions_enabled).to.equal(true);
        expect(leaderState.miner_enabled).to.equal(false);

        // Enable Transactions on Follower
        updateProviderUrl("stratus-follower");
        const enableFollowerTransactionsResponse = await sendAndGetFullResponse("stratus_enableTransactions", []);
        expect(enableFollowerTransactionsResponse.data.result).to.equal(true);
        const followerState = await sendWithRetry("stratus_state", []);
        expect(followerState.transactions_enabled).to.equal(true);

        // Send one Transaction to become pending due to Miner disabled
        updateProviderUrl("stratus");
        const transactionCountResponse = await sendAndGetFullResponse("eth_getTransactionCount", [
            ALICE.address,
            "latest",
        ]);
        const nonce = parseInt(transactionCountResponse.data.result, 16);
        const signedTransaction = await ALICE.signWeiTransfer(BOB.address, 1, nonce);
        const transactionHash = keccak256(signedTransaction);
        const sendTransactionResponse = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTransaction]);
        expect(sendTransactionResponse.data.result).to.equal(transactionHash);

        // Disable transactions on Leader to allow changing miner mode
        const disableTransactionsResponse = await sendAndGetFullResponse("stratus_disableTransactions", []);
        expect(disableTransactionsResponse.data.result).to.equal(false);
        const leaderStateAfterDisable = await sendWithRetry("stratus_state", []);
        expect(leaderStateAfterDisable.transactions_enabled).to.equal(false);

        // Change Miner mode to External on Leader with Pending Transactions should fail
        const changeMinerModeResponse = await sendAndGetFullResponse("stratus_changeMinerMode", ["external"]);
        expect(changeMinerModeResponse.data.error.code).eq(-32603);
        expect(changeMinerModeResponse.data.error.message).eq("There are (1) pending transactions.");

        // Clean up
        updateProviderUrl("stratus");
        const enableTransactionsCleanupResponse = await sendAndGetFullResponse("stratus_enableTransactions", []);
        expect(enableTransactionsCleanupResponse.data.result).to.equal(true);
        const enableMinerCleanupResponse = await sendAndGetFullResponse("stratus_enableMiner", []);
        expect(enableMinerCleanupResponse.data.result).to.equal(true);

        const leaderStateAfterCleanup = await sendWithRetry("stratus_state", []);
        expect(leaderStateAfterCleanup.transactions_enabled).to.equal(true);
        expect(leaderStateAfterCleanup.miner_enabled).to.equal(true);

        await waitForFollowerToSyncWithLeader();

        updateProviderUrl("stratus-follower");
        const disableFollowerTransactionsResponse = await sendAndGetFullResponse("stratus_disableTransactions", []);
        expect(disableFollowerTransactionsResponse.data.result).to.equal(false);
        const disableFollowerMinerCleanupResponse = await sendAndGetFullResponse("stratus_disableMiner", []);
        expect(disableFollowerMinerCleanupResponse.data.result).to.equal(false);

        updateProviderUrl("stratus");
        const disableLeaderTransactionsResponse = await sendAndGetFullResponse("stratus_disableTransactions", []);
        expect(disableLeaderTransactionsResponse.data.result).to.equal(false);
        const disableLeaderMinerCleanupResponse = await sendAndGetFullResponse("stratus_disableMiner", []);
        expect(disableLeaderMinerCleanupResponse.data.result).to.equal(false);
    });

    it("Miner change to External on Leader should succeed", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_changeMinerMode", ["external"]);
        expect(response.data.result).to.equal(true);
        const state = await sendWithRetry("stratus_state", []);
        expect(state.is_interval_miner_shutdown).to.equal(true);
    });

    it("Shutdown importer on Follower", async function () {
        updateProviderUrl("stratus-follower");
        const responseFollower = await sendAndGetFullResponse("stratus_shutdownImporter", []);
        expect(responseFollower.data.result).to.equal(true);
        const state = await sendWithRetry("stratus_state", []);
        expect(state.is_importer_shutdown).to.equal(true);
    });

    it("Miner change to Interval on Follower should succeed", async function () {
        updateProviderUrl("stratus-follower");
        const response = await sendAndGetFullResponse("stratus_changeMinerMode", ["1s"]);
        expect(response.data.result).to.equal(true);
        const state = await sendWithRetry("stratus_state", []);
        expect(state.is_interval_miner_shutdown).to.equal(false);
        expect(state.is_importer_shutdown).to.equal(true);
    });

    it("Validate Follower with Interval Miner is mining a block every second", async function () {
        updateProviderUrl("stratus-follower");

        let initialBlockNumberHex = await sendWithRetry("eth_blockNumber", []);
        let initialBlockNumber = parseInt(initialBlockNumberHex, 16);

        await new Promise((resolve) => setTimeout(resolve, 10000));

        let newBlockNumberHex = await sendWithRetry("eth_blockNumber", []);
        let newBlockNumber = parseInt(newBlockNumberHex, 16);

        expect(newBlockNumber).to.be.greaterThan(initialBlockNumber);
        expect(newBlockNumber - initialBlockNumber).to.be.closeTo(10, 1);
    });
});
