import { expect } from "chai";
import { concat, keccak256, toBeHex } from "ethers";

import { ALICE, BOB, CHARLIE } from "./helpers/account";
import {
    sendAndGetFullResponse,
    sendWithRetry,
    toHex,
    updateProviderUrl,
    waitForFollowerToSyncWithLeader,
} from "./helpers/rpc";

const HASH_ZERO = "0x" + "0".repeat(64);

// Root reported by blocks that mined no transaction.
const EMPTY_TRANSACTIONS_ROOT = "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421";

// How many of the most recent blocks are inspected, so that the suite does not walk a chain of arbitrary length.
const INSPECTED_BLOCKS = 20;

// Recomputes the hash of a mined block from the header fields it reports, mirroring the node.
//
// The preimage is the block number and the timestamp, both as 8 byte big endian integers, followed by the
// 32 byte transactions root and the 32 byte parent hash.
function calculateBlockHash(block: any): string {
    const number = toBeHex(BigInt(block.number), 8);
    const timestamp = toBeHex(BigInt(block.timestamp), 8);
    return keccak256(concat([number, timestamp, block.transactionsRoot, block.parentHash]));
}

// The genesis keeps the legacy scheme, which hashes only the block number.
function calculateGenesisHash(): string {
    return keccak256(toBeHex(0n, 8));
}

async function getBlock(node: string, blockNumber: number): Promise<any> {
    updateProviderUrl(node);
    return await sendWithRetry("eth_getBlockByNumber", [toHex(blockNumber), false]);
}

describe("Leader & Follower block hash integration test", function () {
    // The most recent blocks both nodes agree on, plus the first mined block, which is the oldest one
    // hashed with the current scheme. Set once the follower catches up with the leader.
    let inspectedBlocks: number[] = [];

    it("Validate initial Leader and Follower health", async function () {
        updateProviderUrl("stratus");
        expect(await sendWithRetry("stratus_health", [])).to.equal(true);

        updateProviderUrl("stratus-follower");
        expect(await sendWithRetry("stratus_health", [])).to.equal(true);
    });

    it("Genesis block on the Leader is hashed with the legacy scheme", async function () {
        const genesis = await getBlock("stratus", 0);

        expect(genesis.number).to.equal("0x0");
        expect(genesis.parentHash, "genesis has no parent").to.equal(HASH_ZERO);
        expect(genesis.hash, "genesis hash is the keccak of its number").to.equal(calculateGenesisHash());
    });

    it("Genesis block on the Follower is identical to the one on the Leader", async function () {
        const leaderGenesis = await getBlock("stratus", 0);
        const followerGenesis = await getBlock("stratus-follower", 0);

        expect(followerGenesis, "genesis blocks differ between leader and follower").to.deep.equal(leaderGenesis);
    });

    it("Send transactions to the Leader so that the inspected blocks carry a transactions root", async function () {
        updateProviderUrl("stratus");

        for (const [sender, receiver] of [
            [ALICE, BOB],
            [BOB, CHARLIE],
            [CHARLIE, ALICE],
        ]) {
            const nonce = parseInt(await sendWithRetry("eth_getTransactionCount", [sender.address, "latest"]), 16);
            const signedTx = await sender.signWeiTransfer(receiver.address, 0, nonce);
            const txHash = keccak256(signedTx);

            const response = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
            expect(response.data.result).to.equal(txHash);

            await sendWithRetry("eth_getTransactionReceipt", [txHash]);
        }
    });

    it("Wait for Follower to sync with Leader", async function () {
        const { leaderBlock } = await waitForFollowerToSyncWithLeader();
        const syncedBlock = parseInt(leaderBlock, 16);
        expect(syncedBlock, "chain should have blocks past the genesis").to.be.greaterThan(0);

        const oldest = Math.max(1, syncedBlock - INSPECTED_BLOCKS + 1);
        const recent = Array.from({ length: syncedBlock - oldest + 1 }, (_, i) => oldest + i);
        inspectedBlocks = recent[0] === 1 ? recent : [1, ...recent];
    });

    it("Leader hashes every block over its number, timestamp, transactions root and parent hash", async function () {
        let blocksWithTransactions = 0;

        for (const blockNumber of inspectedBlocks) {
            const block = await getBlock("stratus", blockNumber);

            expect(block.hash, `hash of block ${blockNumber}`).to.equal(calculateBlockHash(block));

            if (block.transactionsRoot !== EMPTY_TRANSACTIONS_ROOT) {
                blocksWithTransactions++;
            }
        }

        // without this the transactions root would be constant and its contribution to the hash unverified
        expect(blocksWithTransactions, "no inspected block mined a transaction").to.be.greaterThan(0);
    });

    it("Leader chains every block to the hash of its parent", async function () {
        for (const blockNumber of inspectedBlocks) {
            const block = await getBlock("stratus", blockNumber);
            const parent = await getBlock("stratus", blockNumber - 1);

            expect(block.parentHash, `parent hash of block ${blockNumber}`).to.equal(parent.hash);
        }
    });

    it("Follower reports the same hashes as the Leader for every block", async function () {
        for (const blockNumber of [0, ...inspectedBlocks]) {
            const leaderBlock = await getBlock("stratus", blockNumber);
            const followerBlock = await getBlock("stratus-follower", blockNumber);

            expect(followerBlock.hash, `hash of block ${blockNumber}`).to.equal(leaderBlock.hash);
            expect(followerBlock.parentHash, `parent hash of block ${blockNumber}`).to.equal(leaderBlock.parentHash);
            expect(followerBlock.timestamp, `timestamp of block ${blockNumber}`).to.equal(leaderBlock.timestamp);
            expect(followerBlock.transactionsRoot, `transactions root of block ${blockNumber}`).to.equal(
                leaderBlock.transactionsRoot,
            );
        }
    });

    it("Follower stores hashes that match the fields of the blocks it imported", async function () {
        for (const blockNumber of inspectedBlocks) {
            const block = await getBlock("stratus-follower", blockNumber);

            expect(block.hash, `hash of block ${blockNumber}`).to.equal(calculateBlockHash(block));
        }
    });

    it("Both nodes resolve blocks by the hash the other one reports", async function () {
        for (const blockNumber of [0, ...inspectedBlocks.slice(-1)]) {
            const leaderBlock = await getBlock("stratus", blockNumber);

            updateProviderUrl("stratus-follower");
            const byHashOnFollower = await sendWithRetry("eth_getBlockByHash", [leaderBlock.hash, false]);
            expect(byHashOnFollower.number, `block ${blockNumber} by hash on follower`).to.equal(leaderBlock.number);

            updateProviderUrl("stratus");
            const byHashOnLeader = await sendWithRetry("eth_getBlockByHash", [leaderBlock.hash, false]);
            expect(byHashOnLeader.number, `block ${blockNumber} by hash on leader`).to.equal(leaderBlock.number);
        }
    });
});
