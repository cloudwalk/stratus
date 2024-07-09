import { expect } from "chai";

import { TEST_ACCOUNTS, randomAccounts } from "../helpers/account";
import { BlockMode, currentBlockMode } from "../helpers/network";
import { send, sendEvmMine, sendGetBalance, sendRawTransactions, sendReset } from "../helpers/rpc";

describe("Multiple Transactions Per Block", () => {
    before(() => {
        expect(currentBlockMode()).eq(BlockMode.External, "Wrong block mining mode is used");
    });

    it("Resets blockchain", async () => {
        // HACK: sleeps for 50ms to avoid having the previous test interfering
        await new Promise((resolve) => setTimeout(resolve, 50));
        await sendReset();
        const blockNumber = await send("eth_blockNumber", []);
        expect(blockNumber).to.be.oneOf(["0x0", "0x1"]);
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

        const latestBlockBeforeMining = await send("eth_getBlockByNumber", ["latest", true]);

        // mine the block
        await sendEvmMine();

        // get the latest block after mining
        const latestBlockAfterMining = await send("eth_getBlockByNumber", ["latest", true]);

        // check if block was mined
        expect(latestBlockAfterMining).to.exist;

        // check if mined block is different from the latest block before mining
        expect(latestBlockAfterMining.hash).to.not.equal(latestBlockBeforeMining.hash);

        // check if all transactions are in the block
        for (let txHash of txHashes) {
            expect(latestBlockAfterMining.transactions.map((tx: any) => tx.hash)).to.include(txHash);
        }

        // check if transactions receipt are valid
        for (let txHash of txHashes) {
            const receipt = await send("eth_getTransactionReceipt", [txHash]);
            expect(receipt).to.exist;
            expect(receipt.transactionHash).to.equal(txHash);

            expect(receipt.blockHash).to.equal(latestBlockAfterMining.hash);
            expect(receipt.blockNumber).to.equal(latestBlockAfterMining.number);
            expect(receipt.contractAddress).to.be.null;
            expect(receipt.cumulativeGasUsed).to.be.a("string");
            expect(receipt.from).to.be.a("string");
            expect(receipt.gasUsed).to.be.a("string");
            expect(receipt.logs).to.be.an("array");
            expect(receipt.logsBloom).to.be.a("string");
            expect(receipt.status).to.be.a("string");
            expect(receipt.to).to.be.a("string");
            expect(receipt.transactionIndex).to.be.a("string");
        }

        // check counterParty balance
        expect(await sendGetBalance(counterParty.address)).eq(expectedCounterPartyBalance);
    });
});
