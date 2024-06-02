import { expect } from "chai";
import { keccak256, TransactionResponse } from "ethers";

import { ALICE, BOB } from "../../helpers/account";
import { BlockMode, currentBlockMode, currentMiningIntervalInMs } from "../../helpers/network";
import { ETHERJS, pollForNextBlock, sendGetNonce, sendRawTransaction, } from "../../helpers/rpc";
import { checkTimeEnough } from "../../helpers/misc";

describe("Known issues for the interval block mining mode. Pending transactions", async () => {
    let blockIntervalInMs: number;

    before(async () => {
        expect(currentBlockMode()).eq(
            BlockMode.Interval,
            "Wrong block mining mode is used"
        );
        blockIntervalInMs = currentMiningIntervalInMs() ?? 0;
        expect(blockIntervalInMs).gte(
            1000,
            "Block interval must be at least 1000 ms"
        );
    });

    it("Can be fetched by the hash before and after minting", async () => {
        const amount = 1;
        const nonce = await sendGetNonce(ALICE);

        const signedTx = await ALICE.signWeiTransfer(BOB.address, amount, nonce);
        const txHash = keccak256(signedTx);

        await pollForNextBlock();
        const txSendingTime = Date.now();
        await sendRawTransaction(signedTx);
        const txResponseAfterSending: TransactionResponse | null = await ETHERJS.getTransaction(txHash);
        checkTimeEnough(txSendingTime, blockIntervalInMs * 1.5);

        await pollForNextBlock();
        const txResponseAfterMinting: TransactionResponse | null = await ETHERJS.getTransaction(txHash);

        expect(txResponseAfterSending).exist;
        expect(txResponseAfterMinting).exist;
        expect(txResponseAfterSending?.hash).eq(txHash);
        expect(txResponseAfterMinting?.hash).eq(txHash);
    });
});
