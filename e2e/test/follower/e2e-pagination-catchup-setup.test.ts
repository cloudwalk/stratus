import { expect } from "chai";
import fs from "fs";

import { ALICE } from "../helpers/account";
import { CHAIN_ID_DEC, send } from "../helpers/rpc";
import { waitForReceipt } from "./helpers";

// Part 1 of the catch-up scenario, run by `just e2e-leader-follower-pagination-catchup` BEFORE
// the follower starts: mines several fat blocks and persists them for the verify step.

const FAT_TXS = 3;
const FAT_TX_DATA_BYTES = 50_000;
const ARTIFACT_FILE = "pagination-catchup.json";

describe("Pagination catch-up (setup)", () => {
    it("mines fat blocks while the follower is down", async () => {
        const blocks = [];
        for (let i = 0; i < FAT_TXS; i++) {
            // fat contract deployment: the code always fails, but the fat data makes the response oversized
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

        fs.writeFileSync(
            ARTIFACT_FILE,
            JSON.stringify({ blocks: blocks, target: blocks[blocks.length - 1].blockNumber }),
        );
    });
});
