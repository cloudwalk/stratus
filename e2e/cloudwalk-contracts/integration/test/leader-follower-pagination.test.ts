import { expect } from "chai";
import { keccak256 } from "ethers";

import { ALICE, randomAccounts } from "./helpers/account";
import {
    send,
    sendAndGetFullResponse,
    sendWithRetry,
    updateProviderUrl,
    waitForFollowerToSyncWithLeader,
} from "./helpers/rpc";

const PAGE_LIMIT = 256;
const TX_COUNT = 300;

describe("Leader & Follower pagination round-trip with stratus_getBlockWithChanges", function () {
    it("Validate initial Leader and Follower health", async function () {
        updateProviderUrl("stratus");
        const leaderHealth = await sendWithRetry("stratus_health", []);
        expect(leaderHealth).to.equal(true);

        updateProviderUrl("stratus-follower");
        const followerHealth = await sendWithRetry("stratus_health", []);
        expect(followerHealth).to.equal(true);
    });

    let blockHash: string;

    it("Sends 300 transactions to Leader to create a block requiring pagination", async function () {
        this.timeout(120000);

        updateProviderUrl("stratus");

        // Pause the miner so all pending transactions land in a single block when resumed
        await sendWithRetry("stratus_disableMiner", []);

        // Send 300 transfer transactions from ALICE to random accounts
        const recipients = randomAccounts(TX_COUNT);
        const nonceHex = await sendWithRetry("eth_getTransactionCount", [ALICE.address, "latest"]);
        const baseNonce = parseInt(nonceHex, 16);

        // Submit all transactions sequentially — Stratus validates nonces eagerly
        for (let i = 0; i < TX_COUNT; i++) {
            const signedTx = await ALICE.signWeiTransfer(recipients[i].address, 0, baseNonce + i);
            const response = await sendAndGetFullResponse("eth_sendRawTransaction", [signedTx]);
            expect(response.data.result).to.equal(keccak256(signedTx));
        }

        // Resume mining — all 300 pending transactions should be committed in one block
        await sendWithRetry("stratus_enableMiner", []);

        // Wait for the block with 300 transactions to be mined
        const targetBlock = await waitForBlockWithTxCount(TX_COUNT, 30);
        expect(targetBlock.transactions, "Block should contain all sent transactions").to.have.length(TX_COUNT);
        blockHash = targetBlock.hash;
        console.log(`Created block ${blockHash} with ${TX_COUNT} transactions`);
    });

    it("Wait for Follower to sync with Leader", async function () {
        this.timeout(120000);
        await waitForFollowerToSyncWithLeader();
    });

    it("Verifies paginated stratus_getBlockWithChanges reassembles identically on Leader and Follower", async function () {
        this.timeout(60000);

        // Fetch all pages from the leader
        updateProviderUrl("stratus");
        const leaderResult = await fetchAllPages(blockHash);

        // Fetch all pages from the follower
        updateProviderUrl("stratus-follower");
        const followerResult = await fetchAllPages(blockHash);

        // Both should require multiple pages
        expect(leaderResult.total, "Total items should exceed page limit to test multi-page").to.be.greaterThan(
            PAGE_LIMIT,
        );
        expect(leaderResult.pageCount, "Should require more than one page").to.be.greaterThan(1);

        // Verify pagination metadata is consistent on both nodes
        expect(followerResult.total, "Follower total should match leader total").to.equal(leaderResult.total);
        expect(followerResult.pageCount, "Follower page count should match leader").to.equal(leaderResult.pageCount);
        expect(followerResult.cursors, "Follower cursor sequence should match leader").to.deep.equal(
            leaderResult.cursors,
        );

        // Compare raw block and changes data (deep equality covers byte arrays, hashes, etc.)
        expect(followerResult.blockData, "Follower block data should match leader block data").to.deep.equal(
            leaderResult.blockData,
        );
        expect(followerResult.changesData, "Follower changes data should match leader changes data").to.deep.equal(
            leaderResult.changesData,
        );

        console.log(
            `Leader: ${leaderResult.pageCount} pages, ${leaderResult.total} total items, ` +
                `${leaderResult.blockData.length} block entries, ${leaderResult.changesData.length} changes entries`,
        );
        console.log(
            `Follower: ${followerResult.pageCount} pages, ${followerResult.total} total items, ` +
                `${followerResult.blockData.length} block entries, ${followerResult.changesData.length} changes entries`,
        );
    });
});

interface PaginatedResult {
    total: number;
    pageCount: number;
    cursors: (string | null)[];
    blockData: any[];
    changesData: any[];
}

/// Fetches all pages for a block via stratus_getBlockWithChanges, collecting raw data for comparison.
async function fetchAllPages(blockHash: string): Promise<PaginatedResult> {
    let cursor: string | null = null;
    let pageCount = 0;
    let total = 0;
    const cursors: (string | null)[] = [];
    const blockData: any[] = [];
    const changesData: any[] = [];

    do {
        const params =
            cursor === null
                ? [blockHash, { cursor: null, limit: PAGE_LIMIT }]
                : [blockHash, { cursor, limit: PAGE_LIMIT }];
        const page = await send("stratus_getBlockWithChanges", params);
        pageCount++;
        total = page.pagination.total;
        cursors.push(page.pagination.nextCursor);

        if (page.block) {
            blockData.push(page.block);
        }
        if (page.changes) {
            changesData.push(page.changes);
        }

        cursor = page.pagination.nextCursor;
    } while (cursor !== null);

    return { total, pageCount, cursors, blockData, changesData };
}

/// Polls until a block with at least `txCount` transactions is mined, with a `maxAttempts` timeout.
async function waitForBlockWithTxCount(txCount: number, maxAttempts: number): Promise<any> {
    for (let attempt = 0; attempt < maxAttempts; attempt++) {
        await new Promise((resolve) => setTimeout(resolve, 1000));
        const blockNumber = await sendWithRetry("eth_blockNumber", []);
        const block = await send("eth_getBlockByNumber", [blockNumber, true]);
        if (block && block.transactions && block.transactions.length >= txCount) {
            return block;
        }
    }
    throw new Error(`Block with ${txCount} transactions was not mined after ${maxAttempts} attempts`);
}
