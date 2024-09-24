import { expect } from "chai";

import { TEST_ACCOUNTS, randomAccounts } from "../helpers/account";
import { pollReceipts, send, sendEvmMine, sendGetBalance, sendRawTransactions, sendReset } from "../helpers/rpc";

describe("Transaction: parallel transfer", () => {
    it("Resets blockchain", async () => {
        // HACK: sleeps for 50ms to avoid having the previous test interfering
        await new Promise((resolve) => setTimeout(resolve, 50));
        await sendReset();
        const blockNumber = await send("eth_blockNumber", []);
        expect(blockNumber).to.be.oneOf(["0x0", "0x1"]);
    });

    it("Sends parallel requests", async () => {
        const counterParty = randomAccounts(1)[0];
        expect(await sendGetBalance(counterParty.address)).eq(0, "counterParty initial balance mismatch");

        // sign transactions from accounts that have balance
        let expectedCounterPartyBalance = 0;
        const signedTxs = [];
        for (let i = 0; i < TEST_ACCOUNTS.length; i++) {
            const amount = i + 1;
            const account = TEST_ACCOUNTS[i];
            signedTxs.push(await account.signWeiTransfer(counterParty.address, amount));
            expectedCounterPartyBalance += amount;
        }

        // sign transaction from accounts that have no balance
        for (const account of randomAccounts(100)) {
            signedTxs.push(await account.signWeiTransfer(counterParty.address, 0));
        }

        // send transactions in parallel
        const hashes = await sendRawTransactions(signedTxs);
        await sendEvmMine();
        const receipts = await pollReceipts(hashes);

        // verify
        expect(receipts.successCount).eq(signedTxs.length, "Success transaction count mismatch");
        expect(receipts.failedCount).eq(0, "Failed transaction count mismatch");

        expect(await sendGetBalance(counterParty.address)).eq(
            expectedCounterPartyBalance,
            "counterParty final balance mismatch",
        );
    });
});
