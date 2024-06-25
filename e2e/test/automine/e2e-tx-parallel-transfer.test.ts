import { expect } from "chai";

import { TEST_ACCOUNTS, randomAccounts } from "../helpers/account";
import { send, sendGetBalance, sendRawTransactions, sendReset } from "../helpers/rpc";

describe("Transaction: parallel transfer", () => {
    it("Resets blockchain", async () => {
        await sendReset();
        const blockNumber = await send("eth_blockNumber", []);
        expect(blockNumber).to.be.oneOf(["0x0", "0x1"]);
    });
    it("Sends parallel requests", async () => {
        const counterParty = randomAccounts(1)[0];
        expect(await sendGetBalance(counterParty.address)).eq(0);

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
        await sendRawTransactions(signedTxs);

        // verify
        expect(await sendGetBalance(counterParty.address)).eq(expectedCounterPartyBalance);
    });
});
