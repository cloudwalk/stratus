import { expect } from "chai";

import { ALICE, BOB, CHARLIE, TEST_ACCOUNTS, randomAccounts } from "../helpers/account";
import { send, sendEvmMine, sendGetBalance, sendRawTransaction, sendReset, updateProviderUrl } from "../helpers/rpc";

describe("Relayer Test", () => {
    it("Resets blockchain", async () => {
        await sendReset();
        const stratusLatestBlock = await send('eth_getBlockByNumber', ['latest', true]);
        console.log('stratusLatestBlock', stratusLatestBlock);

        updateProviderUrl('http://localhost:8545');
        await send("hardhat_reset");
        const hardhatLatestBlock = await send('eth_getBlockByNumber', ['latest', true]);
        console.log('hardhatLatestBlock', hardhatLatestBlock);

        updateProviderUrl('http://localhost:3000');
    });

    it("Send Multiple Txs", async () => {
        const accounts = [ALICE, BOB, CHARLIE];
        const nonces = [0, 0, 0];
        let txHashes = [];

        for (let i = 0; i < 1; i++) {
            // Randomly select the sender and the receiver
            const senderIndex = Math.floor(Math.random() * accounts.length);
            let receiverIndex;
            do {
                receiverIndex = Math.floor(Math.random() * accounts.length);
            } while (receiverIndex === senderIndex);

            const sender = accounts[senderIndex];
            const receiver = accounts[receiverIndex];

            const signedTx = await sender.signWeiTransfer(receiver.address, 1, nonces[senderIndex]);
            console.log('signedTx', signedTx);

            const txHash = await sendRawTransaction(signedTx);
            txHashes.push(txHash);
            console.log('txHash', txHash);

            nonces[senderIndex]++;
        }

        await sendEvmMine();

        const stratusBalanceAlice = await sendGetBalance(ALICE.address);
        console.log('stratusBalanceAlice', stratusBalanceAlice);

        const stratusBalanceBob = await sendGetBalance(BOB.address);
        console.log('stratusBalanceBob', stratusBalanceBob);

        const stratusBalanceCharlie = await sendGetBalance(CHARLIE.address);
        console.log('stratusBalanceCharlie', stratusBalanceCharlie);

        const latestStratusBlock = await send('eth_getBlockByNumber', ['latest', true]);
        console.log('latestStratusBlock', latestStratusBlock);

        updateProviderUrl('http://localhost:8545');

        const transaction = await send('eth_getTransactionByHash', [txHashes[0]]);
        console.log('Transaction:', transaction);

        const hardhatBalanceAlice = await sendGetBalance(ALICE.address);
        console.log('hardhatBalanceAlice', hardhatBalanceAlice);

        const hardhatBalanceBob = await sendGetBalance(BOB.address);
        console.log('hardhatBalanceBob', hardhatBalanceBob);

        const hardhatBalanceCharlie = await sendGetBalance(CHARLIE.address);
        console.log('hardhatBalanceCharlie', hardhatBalanceCharlie);

        const latestHardhatBlock = await send('eth_getBlockByNumber', ['latest', true]);
        console.log('latestHardhatBlock', latestHardhatBlock);
    });

    /* it("Validate Nonces", async () => {
        const stratusNonceAlice = await send('eth_getTransactionCount', [ALICE.address, 'latest']);
        console.log('stratusNonceAlice', stratusNonceAlice);
        

        updateProviderUrl('http://localhost:8545');

        const hardhatNonceAlice = await send('eth_getTransactionCount', [ALICE.address, 'latest']);
        console.log('hardhatNonceAlice', hardhatNonceAlice);
    });

    it("Send Txs", async () => {
        const EMPTY_ACCOUNT = randomAccounts(1)[0];
        let signedTx = await ALICE.signWeiTransfer(EMPTY_ACCOUNT.address, 192192)

        const txHash = await sendRawTransaction(signedTx);
        console.log('txHash', txHash);

        await sendEvmMine();

        const receipt = await send('eth_getTransactionReceipt', [txHash]);
        console.log('receipt', receipt);

        const stratusNonceAlice = await send('eth_getTransactionCount', [ALICE.address, 'latest']);
        console.log('stratusNonceAlice', stratusNonceAlice);
        const stratusBalanceAlice = await sendGetBalance(ALICE.address);
        console.log('stratusBalanceAlice', stratusBalanceAlice);

        const stratusNonceTestAcc = await send('eth_getTransactionCount', [EMPTY_ACCOUNT.address, 'latest']);
        console.log('stratusNonceTestAcc', stratusNonceTestAcc);
        const stratusBalanceTestAcc = await sendGetBalance(EMPTY_ACCOUNT.address);
        console.log('stratusBalanceTestAcc', stratusBalanceTestAcc);
        
        updateProviderUrl('http://localhost:8545');

        const hardhatNonceAlice = await send('eth_getTransactionCount', [ALICE.address, 'latest']);
        console.log('hardhatNonceAlice', hardhatNonceAlice);
        const hardhatBalance = await sendGetBalance(ALICE.address);
        console.log('hardhatBalance', hardhatBalance);

        const hardhatNonceTestAcc = await send('eth_getTransactionCount', [EMPTY_ACCOUNT.address, 'latest']);
        console.log('hardhatNonceTestAcc', hardhatNonceTestAcc);
        const hardhatBalanceTestAcc = await sendGetBalance(EMPTY_ACCOUNT.address);
        console.log('hardhatBalanceTestAcc', hardhatBalanceTestAcc);
    }); */
});