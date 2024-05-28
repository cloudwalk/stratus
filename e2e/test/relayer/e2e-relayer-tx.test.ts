import { expect } from "chai";

import { ALICE, BOB, CHARLIE, randomAccounts } from "../helpers/account";
import { send, sendEvmMine, sendGetBalance, sendRawTransaction, sendReset, updateProviderUrl } from "../helpers/rpc";

describe("Relayer Test", () => {
    it("Validate Balance", async () => {
        const duration = 30;
        const tps = 30;
        const totalTransactions = duration * tps;

        const accounts = [ALICE, BOB, CHARLIE];
        const nonces = [0, 0, 0];

        const signedTxs = [];

        // Create all transactions
        for (let i = 0; i < totalTransactions; i++) {
            const senderIndex = i % accounts.length;
            const receiverIndex = (i + 1) % accounts.length;

            const sender = accounts[senderIndex];
            const receiver = accounts[receiverIndex];

            const randomWei = Math.floor(Math.random() * 1000) + 1;
            const signedTx = await sender.signWeiTransfer(receiver.address, randomWei, nonces[senderIndex]);
            signedTxs.push(signedTx);

            nonces[senderIndex]++;
        }

        let lastTxTime = Date.now();

        // Send all transactions
        for (let i = 0; i < signedTxs.length; i++) {
            const now = Date.now();
            const timeSinceLastTx = now - lastTxTime;
            const desiredInterval = 1000 / tps;

            if (timeSinceLastTx < desiredInterval) {
                const sleepTime = desiredInterval - timeSinceLastTx;
                await sleep(sleepTime);
            }

            await sendRawTransaction(signedTxs[i]);
            lastTxTime = Date.now();
            console.log('Tx number and current time', i, new Date().toISOString());
        }

        await sleep(2000);

        const aliceStratusBalance = await sendGetBalance(ALICE.address);
        const bobStratusBalance = await sendGetBalance(BOB.address);
        const charlieStratusBalance = await sendGetBalance(CHARLIE.address);
        console.log('aliceStratusBalance', aliceStratusBalance);
        console.log('bobStratusBalance', bobStratusBalance);
        console.log('charlieStratusBalance', charlieStratusBalance);

        updateProviderUrl('http://localhost:8545');

        await sleep(2000);

        const aliceHardhatBalance = await sendGetBalance(ALICE.address);
        const bobHardhatBalance = await sendGetBalance(BOB.address);
        const charlieHardhatBalance = await sendGetBalance(CHARLIE.address);
        console.log('aliceHardhatBalance', aliceHardhatBalance);
        console.log('bobHardhatBalance', bobHardhatBalance);
        console.log('charlieHardhatBalance', charlieHardhatBalance);

        expect(aliceStratusBalance).to.equal(aliceHardhatBalance);
        expect(bobStratusBalance).to.equal(bobHardhatBalance);
        expect(charlieStratusBalance).to.equal(charlieHardhatBalance);
});
/* 
    it("Validate 1 Tx Relayed", async () => {
        const sender = ALICE;
        const receiver = BOB;
        const nonce = 0;

        const signedTx = await sender.signWeiTransfer(receiver.address, 1, nonce);
        const txHash = await sendRawTransaction(signedTx);
        console.log('txHash', txHash);

        // Mine Stratus block
        await sendEvmMine();

        const latestStratusBlock = await send('eth_getBlockByNumber', ['latest', true]);
        console.log('latestStratusBlock', latestStratusBlock);

        // Wait for relay
        await sleep(1000);

        updateProviderUrl('http://localhost:8545');

        // Mine Hardhat block
        await sendEvmMine();

        const latestHardhatBlock = await send('eth_getBlockByNumber', ['latest', true]);
        console.log('latestHardhatBlock', latestHardhatBlock);
    });
*/

/*     it("Validate 30 Txs Relayed", async () => {
        const accounts = [ALICE, BOB];
        const nonces = [0, 0];
        const totalTransactions = 30;

        for (let i = 0; i < totalTransactions; i++) {
            const senderIndex = i % 2;
            const receiverIndex = (i + 1) % 2;

            const sender = accounts[senderIndex];
            const receiver = accounts[receiverIndex];

            const signedTx = await sender.signWeiTransfer(receiver.address, 1, nonces[senderIndex]);
            const txHash = await sendRawTransaction(signedTx);
            console.log('txHash', txHash);

            nonces[senderIndex]++;
        }

        // Mine Stratus block
        await sendEvmMine();

        const latestStratusBlock = await send('eth_getBlockByNumber', ['latest', true]);
        console.log('latestStratusBlock', latestStratusBlock);

        // Wait for relay
        await sleep(1000);

        updateProviderUrl('http://localhost:8545');

        // Mine Hardhat block
        await sendEvmMine();

        const latestHardhatBlock = await send('eth_getBlockByNumber', ['latest', true]);
        console.log('latestHardhatBlock', latestHardhatBlock);
    });
*/
});

function sleep(ms: number) {
    return new Promise(resolve => setTimeout(resolve, ms));
}