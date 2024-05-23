import { expect } from "chai";

import { TEST_ACCOUNTS, randomAccounts } from "../helpers/account";
import { send, sendGetBalance, sendRawTransactions, sendReset } from "../helpers/rpc";

describe("Multiple Transactions Per Block", () => {
    it("Resets blockchain", async () => {
        await sendReset();
    });
    it("Send multiple transactions and mine block", async () => {
        const counterParty = randomAccounts(1)[0];
        expect(await sendGetBalance(counterParty.address)).eq(0);

        let expectedCounterPartyBalance = 0;
        const signedTxs = [];
        for (let i = 0; i < TEST_ACCOUNTS.length; i++) {
            const amount = i + 1;
            const account = TEST_ACCOUNTS[i];
            signedTxs.push(await account.signWeiTransfer(counterParty.address, amount));
            expectedCounterPartyBalance += amount;
        }

        for (const account of randomAccounts(100)) {
            signedTxs.push(await account.signWeiTransfer(counterParty.address, 0));
        }

        const txHashes = await sendRawTransactions(signedTxs);

        const latestBlockBeforeMining = await send('eth_getBlockByNumber', ['latest', true]);

        // mine the block
        await send("evm_mine", []);

        // get the latest block after mining
        const latestBlockAfterMining = await send('eth_getBlockByNumber', ['latest', true]);

        // check if block was mined
        expect(latestBlockAfterMining).to.exist;

        // check if mined block is different from the latest block before mining
        expect(latestBlockAfterMining.hash).to.not.equal(latestBlockBeforeMining.hash);

        // check if all transactions are in the block
        for (let txHash of txHashes) {
            expect(latestBlockAfterMining.transactions.map((tx: any) => tx.hash)).to.include(txHash);
        }

        // check counterParty balance
        expect(await sendGetBalance(counterParty.address)).eq(expectedCounterPartyBalance);
    });
});