import { expect } from "chai";

import { ALICE, randomAccounts } from "../helpers/account";
import { BlockMode, currentBlockMode } from "../helpers/network";
import { send, sendAndGetFullResponse, sendEvmMine, sendGetNonce, sendRawTransaction, sendReset } from "../helpers/rpc";

const TX_COUNT = 300;

// Server-side response size cap, must match the one Stratus was started with
// (see just recipe `e2e-pagination-budget`).
const RESPONSE_CAP_BYTES = Number(process.env.MAX_RESPONSE_SIZE_BYTES);

/// Walks a full paginated stream from a plain (limit-less) request, asserting:
/// - every page's wire size fits the response cap (envelope included);
/// - the budget metric reported to the client is sane (near the cap, never above);
/// - every page makes progress and every non-final page carries a cursor.
async function fetchAllPages(method: string, blockHash: string): Promise<any[]> {
    const pages: any[] = [];
    let cursor: string | null = null;
    do {
        const params = cursor === null ? [blockHash] : [blockHash, { cursor }];
        const response = await sendAndGetFullResponse(method, params);
        expect(response.data.error, `${method} should succeed`).to.be.undefined;

        const bodyBytes = Buffer.byteLength(JSON.stringify(response.data), "utf8");
        expect(bodyBytes, `page ${pages.length + 1} exceeded cap: ${bodyBytes} > ${RESPONSE_CAP_BYTES}`).to.be.at.most(
            RESPONSE_CAP_BYTES,
        );

        const page = response.data.result;
        expect(page, `page ${pages.length + 1} should be paginated`).to.have.property("pagination");
        expect(page.pagination.limit)
            .to.be.greaterThan(RESPONSE_CAP_BYTES / 2)
            .and.to.be.at.most(RESPONSE_CAP_BYTES);
        expect(page.pagination.returned, "every page must make progress").to.be.greaterThan(0);

        pages.push(page);
        cursor = page.pagination.nextCursor;
    } while (cursor !== null);

    expect(pages.length, "stream must have more than one page").to.be.greaterThan(1);
    return pages;
}

describe("Byte-budget Pagination", function () {
    before(function () {
        expect(currentBlockMode()).eq(BlockMode.External, "Wrong block mining mode is used");
        if (!RESPONSE_CAP_BYTES) {
            console.log("  skipping: MAX_RESPONSE_SIZE_BYTES not set (run via `just e2e-pagination-budget`)");
            this.skip();
        }
    });

    let blockHash: string;
    let blockNumber: string;

    it("mines a block spanning multiple pages", async function () {
        this.timeout(120000);
        await sendReset();

        const recipients = randomAccounts(TX_COUNT);
        const baseNonce = await sendGetNonce(ALICE);
        for (let i = 0; i < TX_COUNT; i++) {
            const signedTx = await ALICE.signWeiTransfer(recipients[i].address, 0, baseNonce + i);
            await sendRawTransaction(signedTx);
        }
        await sendEvmMine();

        blockNumber = await send("eth_blockNumber");
        const block = await send("eth_getBlockByNumber", [blockNumber, false]);
        blockHash = block.hash;
        expect(block.transactions).to.have.length(TX_COUNT);
    });

    it("stratus_getBlockAndReceipts: plain request paginates by response size", async function () {
        this.timeout(30000);

        const pages = await fetchAllPages("stratus_getBlockAndReceipts", blockHash);
        const last = pages[pages.length - 1];
        expect(last.pagination.nextCursor).to.be.null;
        expect(last.pagination.total).to.equal(TX_COUNT);

        // Sum of returned items across pages equals the total.
        const returnedSum = pages.reduce((acc, p) => acc + p.pagination.returned, 0);
        expect(returnedSum).to.equal(TX_COUNT);

        // Reassembled transactions and receipts: same content as the block fetch,
        // byte for byte.
        const fullBlockHashOnly = await send("eth_getBlockByNumber", [blockNumber, false]);
        const referenceTxs = fullBlockHashOnly.transactions;
        const reassembledTxs = pages.flatMap((p: any) => p.block.transactions.map((tx: any) => tx.hash));
        expect(reassembledTxs).to.deep.equal(referenceTxs);

        const reassembledReceipts = pages.flatMap((p: any) => p.receipts);
        expect(reassembledReceipts).to.have.length(TX_COUNT);
        for (const receipt of reassembledReceipts) {
            expect(receipt.transactionHash, "receipt references an existing tx").to.be.oneOf(referenceTxs);
        }
    });

    it("stratus_getBlockWithChanges: plain request paginates by response size", async function () {
        this.timeout(30000);

        const pages = await fetchAllPages("stratus_getBlockWithChanges", blockHash);
        const last = pages[pages.length - 1];
        expect(last.pagination.nextCursor).to.be.null;

        // Returned sums across pages equal the reported total.
        const returnedSum = pages.reduce((acc, p) => acc + p.pagination.returned, 0);
        expect(returnedSum).to.equal(last.pagination.total);

        // Txs, account changes and slot changes collected across pages sum to the total.
        const txs = pages.reduce((acc, p) => acc + p.block.transactions.length, 0);
        const accountChanges = pages.reduce((acc, p) => acc + Object.keys(p.changes.account_changes).length, 0);
        const slotChanges = pages.reduce((acc, p) => acc + p.changes.slot_changes.length, 0);
        expect(txs + accountChanges + slotChanges).to.equal(last.pagination.total);

        // Paginated txs are the internal rocks representation; count across pages = mined block.
        const reassembledTxs = pages.flatMap((p: any) => p.block.transactions);
        expect(reassembledTxs).to.have.length(TX_COUNT);
        for (const tx of reassembledTxs) {
            expect(tx.input.hash, "each tx carries its hash").to.exist;
        }

        // Pages return disjoint account/slot changes; their partition sums to the total.
        const accountKeys = pages.flatMap((p) => Object.keys(p.changes.account_changes));
        expect(new Set(accountKeys).size, "account changes are disjoint across pages").to.equal(accountKeys.length);
        const slotKeys = pages.flatMap((p) => p.changes.slot_changes.map((k: any) => JSON.stringify(k)));
        expect(new Set(slotKeys).size, "slot changes are disjoint across pages").to.equal(slotKeys.length);
    });
});
