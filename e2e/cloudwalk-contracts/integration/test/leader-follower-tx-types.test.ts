import { expect } from "chai";
import { keccak256 } from "ethers";

import { ALICE, BOB, CHARLIE, DAVE, EVE } from "./helpers/account";
import {
    sendAndGetFullResponse,
    sendWithRetry,
    updateProviderUrl,
    waitForFollowerToSyncWithLeader,
} from "./helpers/rpc";

describe("Leader & Follower transaction types signer recovery regression test", function () {
    it("Validate initial Leader and Follower health", async function () {
        updateProviderUrl("stratus");
        const leaderHealth = await sendWithRetry("stratus_health", []);
        expect(leaderHealth).to.equal(true);

        updateProviderUrl("stratus-follower");
        const followerHealth = await sendWithRetry("stratus_health", []);
        expect(followerHealth).to.equal(true);
    });

    let legacyHash: string;
    it("Send Type 0 (Legacy) transaction with all signature fields to Leader", async function () {
        updateProviderUrl("stratus");
        const nonceHex = await sendWithRetry("eth_getTransactionCount", [ALICE.address, "latest"]);
        const nonce = parseInt(nonceHex, 16);
        console.log("ALICE nonce (hex):", nonceHex, "ALICE nonce (decimal):", nonce);
        const signedTx = await ALICE.signFullFieldsLegacy(BOB.address, 1, nonce);
        legacyHash = keccak256(signedTx);
        console.log("Type 0 (Legacy) transaction hash:", legacyHash);
        const response = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(response.data.result).to.equal(legacyHash);
    });

    let eip2930Hash: string;
    it("Send Type 1 (EIP-2930) transaction with non-empty access list to Leader", async function () {
        updateProviderUrl("stratus");
        const nonceHex = await sendWithRetry("eth_getTransactionCount", [DAVE.address, "latest"]);
        const nonce = parseInt(nonceHex, 16);
        console.log("DAVE nonce (hex):", nonceHex, "DAVE nonce (decimal):", nonce);
        const signedTx = await DAVE.signFullFieldsEIP2930(ALICE.address, 1, nonce);
        eip2930Hash = keccak256(signedTx);
        console.log("Type 1 (EIP-2930) transaction hash:", eip2930Hash);
        const response = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(response.data.result).to.equal(eip2930Hash);
    });

    let eip1559Hash: string;
    it("Send Type 2 (EIP-1559) transaction with non-empty access list and distinct fee fields to Leader", async function () {
        updateProviderUrl("stratus");
        const nonceHex = await sendWithRetry("eth_getTransactionCount", [CHARLIE.address, "latest"]);
        const nonce = parseInt(nonceHex, 16);
        console.log("CHARLIE nonce (hex):", nonceHex, "CHARLIE nonce (decimal):", nonce);
        const signedTx = await CHARLIE.signFullFieldsEIP1559(BOB.address, 1, nonce);
        eip1559Hash = keccak256(signedTx);
        console.log("Type 2 (EIP-1559) transaction hash:", eip1559Hash);
        const response = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(response.data.result).to.equal(eip1559Hash);
    });

    let eip4844Hash: string;
    it("Send Type 3 (EIP-4844) transaction with all blob and access list fields to Leader", async function () {
        updateProviderUrl("stratus");
        const nonceHex = await sendWithRetry("eth_getTransactionCount", [EVE.address, "latest"]);
        const nonce = parseInt(nonceHex, 16);
        console.log("EVE nonce (hex):", nonceHex, "EVE nonce (decimal):", nonce);
        const signedTx = await EVE.signFullFieldsEIP4844(BOB.address, 1, nonce);
        eip4844Hash = keccak256(signedTx);
        console.log("Type 3 (EIP-4844) transaction hash:", eip4844Hash);
        const response = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
        expect(response.data.result).to.equal(eip4844Hash);
    });

    it("Wait for Follower to sync with Leader", async function () {
        await waitForFollowerToSyncWithLeader();
    });

    it("Preserves correct signer for all transaction types with full signature fields on both Leader and Follower", async function () {
        // Verify on Leader
        console.log("Verifying transactions on Leader...");
        updateProviderUrl("stratus");

        console.log("Getting Type 0 (Legacy) tx:", legacyHash);
        const leaderLegacyTx = await sendWithRetry("eth_getTransactionByHash", [legacyHash]);
        expect(leaderLegacyTx.from.toLowerCase()).to.equal(ALICE.address.toLowerCase());
        expect(leaderLegacyTx.type).to.equal("0x0");

        console.log("Getting Type 1 (EIP-2930) tx:", eip2930Hash);
        const leaderEIP2930Tx = await sendWithRetry("eth_getTransactionByHash", [eip2930Hash]);
        expect(leaderEIP2930Tx.from.toLowerCase()).to.equal(DAVE.address.toLowerCase());
        expect(leaderEIP2930Tx.type).to.equal("0x1");

        console.log("Getting Type 2 (EIP-1559) tx:", eip1559Hash);
        const leaderEIP1559Tx = await sendWithRetry("eth_getTransactionByHash", [eip1559Hash]);
        expect(leaderEIP1559Tx.from.toLowerCase()).to.equal(CHARLIE.address.toLowerCase());
        expect(leaderEIP1559Tx.type).to.equal("0x2");

        console.log("Getting Type 3 (EIP-4844) tx:", eip4844Hash);
        const leaderEIP4844Tx = await sendWithRetry("eth_getTransactionByHash", [eip4844Hash]);
        expect(leaderEIP4844Tx.from.toLowerCase()).to.equal(EVE.address.toLowerCase());
        expect(leaderEIP4844Tx.type).to.equal("0x3");

        // Wait for follower to sync
        await waitForFollowerToSyncWithLeader();

        // Verify on Follower
        console.log("Verifying transactions on Follower...");
        updateProviderUrl("stratus-follower");

        console.log("Getting Type 0 (Legacy) tx:", legacyHash);
        const followerLegacyTx = await sendWithRetry("eth_getTransactionByHash", [legacyHash]);
        expect(followerLegacyTx.from.toLowerCase()).to.equal(ALICE.address.toLowerCase());
        expect(followerLegacyTx.type).to.equal("0x0");

        console.log("Getting Type 1 (EIP-2930) tx:", eip2930Hash);
        const followerEIP2930Tx = await sendWithRetry("eth_getTransactionByHash", [eip2930Hash]);
        expect(followerEIP2930Tx.from.toLowerCase()).to.equal(DAVE.address.toLowerCase());
        expect(followerEIP2930Tx.type).to.equal("0x1");

        console.log("Getting Type 2 (EIP-1559) tx:", eip1559Hash);
        const followerEIP1559Tx = await sendWithRetry("eth_getTransactionByHash", [eip1559Hash]);
        expect(followerEIP1559Tx.from.toLowerCase()).to.equal(CHARLIE.address.toLowerCase());
        expect(followerEIP1559Tx.type).to.equal("0x2");

        console.log("Getting Type 3 (EIP-4844) tx:", eip4844Hash);
        const followerEIP4844Tx = await sendWithRetry("eth_getTransactionByHash", [eip4844Hash]);
        expect(followerEIP4844Tx.from.toLowerCase()).to.equal(EVE.address.toLowerCase());
        expect(followerEIP4844Tx.type).to.equal("0x3");

        // Verify receipts on Leader
        console.log("Verifying receipts on Leader...");
        updateProviderUrl("stratus");

        console.log("Getting receipt for Type 0 (Legacy) tx:", legacyHash);
        const leaderLegacyReceipt = await sendWithRetry("eth_getTransactionReceipt", [legacyHash]);
        expect(leaderLegacyReceipt.from.toLowerCase()).to.equal(ALICE.address.toLowerCase());

        console.log("Getting receipt for Type 1 (EIP-2930) tx:", eip2930Hash);
        const leaderEIP2930Receipt = await sendWithRetry("eth_getTransactionReceipt", [eip2930Hash]);
        expect(leaderEIP2930Receipt.from.toLowerCase()).to.equal(DAVE.address.toLowerCase());

        console.log("Getting receipt for Type 2 (EIP-1559) tx:", eip1559Hash);
        const leaderEIP1559Receipt = await sendWithRetry("eth_getTransactionReceipt", [eip1559Hash]);
        expect(leaderEIP1559Receipt.from.toLowerCase()).to.equal(CHARLIE.address.toLowerCase());

        console.log("Getting receipt for Type 3 (EIP-4844) tx:", eip4844Hash);
        const leaderEIP4844Receipt = await sendWithRetry("eth_getTransactionReceipt", [eip4844Hash]);
        expect(leaderEIP4844Receipt.from.toLowerCase()).to.equal(EVE.address.toLowerCase());

        // Verify receipts on Follower
        console.log("Verifying receipts on Follower...");
        updateProviderUrl("stratus-follower");

        console.log("Getting receipt for Type 0 (Legacy) tx:", legacyHash);
        const followerLegacyReceipt = await sendWithRetry("eth_getTransactionReceipt", [legacyHash]);
        expect(followerLegacyReceipt.from.toLowerCase()).to.equal(ALICE.address.toLowerCase());

        console.log("Getting receipt for Type 1 (EIP-2930) tx:", eip2930Hash);
        const followerEIP2930Receipt = await sendWithRetry("eth_getTransactionReceipt", [eip2930Hash]);
        expect(followerEIP2930Receipt.from.toLowerCase()).to.equal(DAVE.address.toLowerCase());

        console.log("Getting receipt for Type 2 (EIP-1559) tx:", eip1559Hash);
        const followerEIP1559Receipt = await sendWithRetry("eth_getTransactionReceipt", [eip1559Hash]);
        expect(followerEIP1559Receipt.from.toLowerCase()).to.equal(CHARLIE.address.toLowerCase());

        console.log("Getting receipt for Type 3 (EIP-4844) tx:", eip4844Hash);
        const followerEIP4844Receipt = await sendWithRetry("eth_getTransactionReceipt", [eip4844Hash]);
        expect(followerEIP4844Receipt.from.toLowerCase()).to.equal(EVE.address.toLowerCase());
    });
});
