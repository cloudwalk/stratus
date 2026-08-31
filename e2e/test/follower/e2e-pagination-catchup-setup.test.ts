import { expect } from "chai";
import fs from "fs";

import { ALICE } from "../helpers/account";
import { CHAIN_ID_DEC, send } from "../helpers/rpc";

// Part 1 of the catch-up scenario, executed by the `just e2e-leader-follower-pagination-catchup`
// recipe BEFORE the follower starts: mines several fat blocks while the follower is down and
// persists them for the verify step. The follower then starts several blocks behind with a small
// response size limit, forcing it to catch up through multiple consecutive paginated responses.

const FAT_TXS = 3;
const FAT_TX_DATA_BYTES = 50_000;
const ARTIFACT_FILE = "pagination-catchup.json";
const POLL_INTERVAL_MS = 500;

describe("Pagination catch-up (setup)", () => {
    it("mines fat blocks while the follower is down", async () => {
        const blocks = [];
        for (let i = 0; i < FAT_TXS; i++) {
            // fat contract-deployment transaction: the code always fails, but the fat data
            // still makes the block + receipts response exceed the leader's response limits
            const nonce = await send("eth_getTransactionCount", [ALICE.address]);
            const signedTx = await ALICE.signer().signTransaction({
                data: "0x" + "ab".repeat(FAT_TX_DATA_BYTES),
                chainId: CHAIN_ID_DEC,
                gasPrice: 0,
                gasLimit: 10_000_000,
                nonce: nonce,
            });
            const txHash = await send("eth_sendRawTransaction", [signedTx]);
            const receipt = await waitForReceipt(txHash);
            blocks.push({ txHash: txHash, blockNumber: parseInt(receipt.blockNumber, 16) });
        }

        // transactions are mined sequentially and must land in distinct, increasing blocks
        for (let i = 1; i < blocks.length; i++) {
            expect(blocks[i].blockNumber).to.be.greaterThan(blocks[i - 1].blockNumber);
        }

        const artifact = JSON.stringify({ blocks: blocks, target: blocks[blocks.length - 1].blockNumber });
        fs.writeFileSync(ARTIFACT_FILE, artifact);
    });
});

// Polls the leader until the transaction is mined and returns its receipt.
async function waitForReceipt(txHash: string): Promise<any> {
    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline) {
        const receipt = await send("eth_getTransactionReceipt", [txHash]);
        if (receipt) {
            return receipt;
        }
        await sleep(POLL_INTERVAL_MS);
    }
    throw new Error(`transaction ${txHash} was not mined in time`);
}

function sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
