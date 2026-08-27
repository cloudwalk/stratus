import { expect } from "chai";

import { ALICE, randomAccounts } from "../helpers/account";
import { BlockMode, currentBlockMode } from "../helpers/network";
import { send, sendEvmMine, sendGetNonce, sendRawTransaction, sendReset } from "../helpers/rpc";

const PAGE_LIMIT = 256;
const TX_COUNT = 300;

describe("Pagination", function () {
    before(() => {
        expect(currentBlockMode()).eq(BlockMode.External, "Wrong block mining mode is used");
    });

    describe("stratus_getBlockAndReceipts", () => {
        it("fetches all data across multiple pages when block needs pagination", async function () {
            this.timeout(60000);
            await sendReset();

            const { blockHash, fullBlock } = await mineBlockWithTransactions(TX_COUNT);
            expect(fullBlock.transactions).to.have.length(TX_COUNT);

            // Fetch page 1
            const page1 = await send("stratus_getBlockAndReceipts", [blockHash, { cursor: null, limit: PAGE_LIMIT }]);
            expect(page1.pagination.returned).to.equal(PAGE_LIMIT);
            expect(page1.pagination.total).to.equal(TX_COUNT);
            expect(page1.pagination.limit).to.equal(PAGE_LIMIT);
            expect(page1.pagination.nextCursor).to.be.a("string");
            expect(page1.pagination.nextCursor).to.match(/^v1:0x[0-9a-f]+:256$/i);
            expect(page1.block.transactions).to.have.length(PAGE_LIMIT);
            expect(page1.receipts).to.have.length(PAGE_LIMIT);

            // Fetch page 2 using the cursor
            const page2 = await send("stratus_getBlockAndReceipts", [
                blockHash,
                { cursor: page1.pagination.nextCursor, limit: PAGE_LIMIT },
            ]);
            const remaining = TX_COUNT - PAGE_LIMIT;
            expect(page2.pagination.returned).to.equal(remaining);
            expect(page2.pagination.total).to.equal(TX_COUNT);
            expect(page2.pagination.nextCursor).to.be.null;
            expect(page2.block.transactions).to.have.length(remaining);
            expect(page2.receipts).to.have.length(remaining);

            // Reassemble: concat tx hashes from both pages and compare against full block
            const page1TxHashes = page1.block.transactions.map((tx: any) => tx.hash);
            const page2TxHashes = page2.block.transactions.map((tx: any) => tx.hash);
            const reassembledTxHashes = [...page1TxHashes, ...page2TxHashes];

            const fullBlockTxHashes = fullBlock.transactions.map((tx: any) => tx.hash);
            expect(reassembledTxHashes).to.deep.equal(fullBlockTxHashes);

            // Reassemble receipts and validate count
            const allReceipts = [...page1.receipts, ...page2.receipts];
            expect(allReceipts).to.have.length(TX_COUNT);
            for (const receipt of allReceipts) {
                expect(receipt.blockHash).to.equal(blockHash);
            }
        });

        it("returns one-shot legacy shape when block fits within limit", async function () {
            await sendReset();

            const { blockHash } = await mineBlockWithTransactions(1);

            // Fits within the limit: server responds with the complete block in one shot,
            // exactly like the legacy response (no pagination field).
            const oneShot = await send("stratus_getBlockAndReceipts", [
                blockHash,
                { cursor: null, limit: PAGE_LIMIT },
            ]);
            expect(oneShot).to.not.have.property("pagination");
            expect(oneShot.block.transactions).to.have.length(1);
            expect(oneShot.receipts).to.have.length(1);

            // Byte-identical to the legacy (no pagination param) response
            const legacy = await send("stratus_getBlockAndReceipts", [blockHash]);
            expect(oneShot).to.deep.equal(legacy);
        });
    });

    describe("stratus_getBlockWithChanges", () => {
        it("fetches all data across multiple pages when block needs pagination", async function () {
            this.timeout(60000);
            await sendReset();

            const { blockHash } = await mineBlockWithTransactions(TX_COUNT);

            // getBlockWithChanges paginates across 3 sections: transactions, account changes, slot changes.
            // Fetch the legacy (unpaginated) response to learn the expected total and reference data.
            const legacy = await send("stratus_getBlockWithChanges", [blockHash]);
            const legacyBlock = legacy[0];
            const legacyChanges = legacy[1];
            const accountChangesCount = Object.keys(legacyChanges.account_changes).length;
            const slotChangesCount = legacyChanges.slot_changes.length;
            const expectedTotal = TX_COUNT + accountChangesCount + slotChangesCount;

            // Fetch page 1
            const page1 = await send("stratus_getBlockWithChanges", [blockHash, { cursor: null, limit: PAGE_LIMIT }]);
            expect(page1.pagination.returned).to.equal(PAGE_LIMIT);
            expect(page1.pagination.total).to.equal(expectedTotal);
            expect(page1.pagination.nextCursor).to.be.a("string");
            expect(page1.block.transactions).to.have.length(PAGE_LIMIT);
            expect(page1.changes).to.not.be.null;

            // Fetch page 2 using the cursor
            const page2 = await send("stratus_getBlockWithChanges", [
                blockHash,
                { cursor: page1.pagination.nextCursor, limit: PAGE_LIMIT },
            ]);
            expect(page2.pagination.total).to.equal(expectedTotal);
            expect(page2.pagination.nextCursor).to.be.null;
            expect(page2.changes).to.not.be.null;

            // Sum of returned across all pages should equal total
            const totalReturned = page1.pagination.returned + page2.pagination.returned;
            expect(totalReturned).to.equal(expectedTotal);

            // Reassemble transactions from both pages and compare against legacy (same BlockRocksdb format)
            const reassembledTxs = [...page1.block.transactions, ...page2.block.transactions];
            expect(reassembledTxs).to.deep.equal(legacyBlock.transactions);
        });

        it("returns one-shot legacy tuple when block fits within limit", async function () {
            await sendReset();

            const { blockHash } = await mineBlockWithTransactions(1);

            // Fits within the limit: server responds with the legacy tuple [block, changes],
            // no pagination field.
            const oneShot = await send("stratus_getBlockWithChanges", [
                blockHash,
                { cursor: null, limit: PAGE_LIMIT },
            ]);
            expect(Array.isArray(oneShot), "one-shot response should be a tuple array").to.be.true;
            expect(oneShot).to.not.have.property("pagination");
            expect(oneShot[0], "first element should be the block").to.not.be.null;
            expect(oneShot[0].transactions).to.have.length(1);
            expect(oneShot[1], "second element should be the changes").to.not.be.null;

            // Byte-identical to the legacy (no pagination param) response
            const legacy = await send("stratus_getBlockWithChanges", [blockHash]);
            expect(oneShot).to.deep.equal(legacy);
        });

        it("returns raw tuple [block, changes] when no pagination param is provided", async function () {
            await sendReset();

            const { blockHash, fullBlock } = await mineBlockWithTransactions(1);

            const response = await send("stratus_getBlockWithChanges", [blockHash]);

            // Legacy response is a tuple [block, changes] serialized as a JSON array,
            // not wrapped in {block, changes, pagination} like the paginated path
            expect(Array.isArray(response), "legacy response should be a tuple array").to.be.true;
            expect(response).to.not.have.property("pagination");
            expect(response[0], "first element should be the block").to.not.be.null;
            expect(response[0].header, "block should have a header").to.not.be.null;
            expect(response[1], "second element should be the changes").to.not.be.null;
        });
    });
});

/// Mines a block with `count` transactions from ALICE to random recipients.
/// Returns the block hash and the full block (with transactions).
async function mineBlockWithTransactions(count: number): Promise<{ blockHash: string; fullBlock: any }> {
    const recipients = randomAccounts(count);
    const baseNonce = await sendGetNonce(ALICE);
    for (let i = 0; i < count; i++) {
        const signedTx = await ALICE.signWeiTransfer(recipients[i].address, 0, baseNonce + i);
        await sendRawTransaction(signedTx);
    }
    await sendEvmMine();

    const blockNumber = await send("eth_blockNumber");
    const fullBlock = await send("eth_getBlockByNumber", [blockNumber, true]);
    return { blockHash: fullBlock.hash, fullBlock };
}
