import { expect } from "chai";
import { keccak256 } from "ethers";

import { ALICE, BOB } from "../helpers/account";
import { CHAIN_ID_DEC } from "../helpers/rpc";
import { FOLLOWER_URL, LEADER_URL, rpcCall, waitForFollowerBlock, waitForReceipt } from "./helpers";

const METHOD = "stratus_sendRawTransactionAndGetReceipt";

describe("stratus_sendRawTransactionAndGetReceipt", () => {
    for (const reverted of [false, true]) {
        it(`forwards a ${reverted ? "reverted" : "successful"} execution receipt before mining`, async () => {
            const nonce = await rpcCall(LEADER_URL, "eth_getTransactionCount", [ALICE.address, "pending"]);
            const signedTx = reverted
                ? await ALICE.signer().signTransaction({
                      data: "0x60006000fd",
                      chainId: CHAIN_ID_DEC,
                      gasPrice: 0,
                      gasLimit: 1_000_000,
                      nonce: Number(nonce.result),
                  })
                : await ALICE.signWeiTransfer(BOB.address, 1, Number(nonce.result));
            let receipt: any;
            try {
                const paused = await rpcCall(LEADER_URL, "stratus_disableMiner");
                expect(paused.result).to.equal(false);
                const blockNumber = await rpcCall(LEADER_URL, "eth_blockNumber");
                const response = await rpcCall(FOLLOWER_URL, METHOD, [signedTx]);
                receipt = response.result;

                expect(receipt, JSON.stringify(response)).to.be.an("object");
                expect(receipt.transactionHash).to.equal(keccak256(signedTx));
                expect(receipt.status).to.equal(reverted ? "0x0" : "0x1");
                expect(receipt.from).to.equal(ALICE.address);
                expect(receipt.to).to.equal(reverted ? null : BOB.address);
                expect((await rpcCall(LEADER_URL, "eth_blockNumber")).result).to.equal(blockNumber.result);
            } finally {
                await rpcCall(LEADER_URL, "stratus_enableMiner");
            }

            const mined = await waitForReceipt(receipt.transactionHash);
            expect(mined.status).to.equal(receipt.status);
            expect(mined.gasUsed).to.equal(receipt.gasUsed);
            expect(mined.logs).to.deep.equal(receipt.logs);
            await waitForFollowerBlock(Number(mined.blockNumber));
            const replicated = await rpcCall(FOLLOWER_URL, "eth_getTransactionReceipt", [receipt.transactionHash]);
            expect(replicated.result).to.deep.equal(mined);
        });
    }

    it("preserves the leader error through follower forwarding", async () => {
        const nonce = await rpcCall(LEADER_URL, "eth_getTransactionCount", [ALICE.address, "pending"]);
        const signedTx = await ALICE.signWeiTransfer(BOB.address, 1, Number(nonce.result));
        try {
            const disabled = await rpcCall(LEADER_URL, "stratus_disableTransactions");
            expect(disabled.result).to.equal(false);
            const direct = await rpcCall(LEADER_URL, METHOD, [signedTx]);
            const forwarded = await rpcCall(FOLLOWER_URL, METHOD, [signedTx]);
            expect(direct.error).to.be.an("object");
            expect(forwarded.error.code).to.equal(direct.error.code);
            expect(forwarded.error.message).to.equal(direct.error.message);
            expect(forwarded.error.data ?? null).to.deep.equal(direct.error.data ?? null);
        } finally {
            await rpcCall(LEADER_URL, "stratus_enableTransactions");
        }
    });
});
