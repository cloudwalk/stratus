import { expect } from "chai";
import { TransactionResponse, keccak256 } from "ethers";

import { ALICE, BOB } from "../helpers/account";
import { BlockMode, currentBlockMode } from "../helpers/network";
import { ETHERJS, sendEvmMine, sendGetNonce, sendRawTransaction } from "../helpers/rpc";

describe("JSON-RPC", () => {
    before(() => {
        expect(currentBlockMode()).eq(BlockMode.External, "Wrong block mining mode is used");
    });

    describe("Transaction", () => {
        describe("eth_getTransactionByHash", () => {
            it("returns transaction before and after minting", async () => {
                const amount = 1;
                const nonce = await sendGetNonce(ALICE);

                const signedTx = await ALICE.signWeiTransfer(BOB.address, amount, nonce);
                const txHash = keccak256(signedTx);
                await sendRawTransaction(signedTx);
                const txResponseAfterSending: TransactionResponse | null = await ETHERJS.getTransaction(txHash);

                await sendEvmMine();
                const txResponseAfterMinting: TransactionResponse | null = await ETHERJS.getTransaction(txHash);

                expect(txResponseAfterSending).exist;
                expect(txResponseAfterMinting).exist;
                expect(txResponseAfterSending?.hash).eq(txHash);
                expect(txResponseAfterMinting?.hash).eq(txHash);
            });
        });
    });
});
