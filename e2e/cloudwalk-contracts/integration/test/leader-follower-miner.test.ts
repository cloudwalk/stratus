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
        expect(leaderNode.is_interval_miner_shutdown).to.equal(false);
        expect(leaderNode.transactions_enabled).to.equal(true);
        const leaderHealth = await sendWithRetry("stratus_health", []);
        expect(leaderHealth).to.equal(true);
    });

    it("Validate initial Follower state and health", async function () {
        updateProviderUrl("stratus-follower");
        const followerNode = await sendWithRetry("stratus_state", []);
        expect(followerNode.is_leader).to.equal(false);
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

    it("Send transaction to Follower should fail when transactions are disabled on Leader", async function () {
        updateProviderUrl("stratus-follower");
        const nonceResponse = await sendAndGetFullResponse("eth_getTransactionCount", [ALICE.address, "latest"]);
        const nonce = parseInt(nonceResponse.data.result, 16);
        const signedTx = await ALICE.signWeiTransfer(BOB.address, 1, nonce);
        const txHash = keccak256(signedTx);
        const txResponse = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(txResponse.data.error.code).eq(-32009);
        expect(txResponse.data.error.message).eq("Transaction processing is temporarily disabled."); // Needs new message(forward)
    });

    it("Disable transactions on Follower", async function () {
        updateProviderUrl("stratus-follower");
        const response = await sendAndGetFullResponse("stratus_disableTransactions", []);
        expect(response.data.result).to.equal(false);
        const state = await sendWithRetry("stratus_state", []);
        expect(state.transactions_enabled).to.equal(false);
    });

    it("Send transaction to Follower should fail when transactions are disabled on Follower", async function () {
        updateProviderUrl("stratus-follower");
        const nonceResponse = await sendAndGetFullResponse("eth_getTransactionCount", [ALICE.address, "latest"]);
        const nonce = parseInt(nonceResponse.data.result, 16);
        const signedTx = await ALICE.signWeiTransfer(BOB.address, 1, nonce);
        const txHash = keccak256(signedTx);
        const txResponse = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(txResponse.data.error.code).eq(-32009);
        expect(txResponse.data.error.message).eq("Transaction processing is temporarily disabled.");
    });

    it("Disable transactions on Leader", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_disableTransactions", []);
        expect(response.data.result).to.equal(false);
        const state = await sendWithRetry("stratus_state", []);
        expect(state.transactions_enabled).to.equal(false);
    });

    it("Miner change on Leader without params should fail", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_changeMinerMode", []);
        expect(response.data.error.code).eq(-32602);
        expect(response.data.error.message).eq("Expected MinerMode parameter, but received nothing.");
    });

    it("Miner change on Leader with invalid params should fail", async function () {
        updateProviderUrl("stratus");
        const response = await sendAndGetFullResponse("stratus_changeMinerMode", ["invalidMode"]);
        expect(response.data.error.code).eq(-32602);
        expect(response.data.error.message).eq("Failed to decode MinerMode parameter.");
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

        updateProviderUrl("stratus");
        const disableLeaderTransactionsResponse = await sendAndGetFullResponse("stratus_disableTransactions", []);
        expect(disableLeaderTransactionsResponse.data.result).to.equal(false);
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
    });
});
