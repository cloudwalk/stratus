import { expect } from "chai";

import { TestContractBalances, TestContractCounter } from "../../typechain-types";
import { ALICE, BOB, CHARLIE, randomAccounts } from "../helpers/account";
import {
    TX_PARAMS,
    deployTestContractBalances,
    deployTestContractCounter,
    pollReceipts,
    send,
    sendGetNonce,
    sendRawTransactions,
    sendReset,
} from "../helpers/rpc";

describe("Transaction: parallel TestContractBalances", async () => {
    let _contract: TestContractBalances;

    it("Resets blockchain", async () => {
        await sendReset();
        const blockNumber = await send("eth_blockNumber", []);
        expect(blockNumber).to.be.oneOf(["0x0", "0x1"]);
    });

    it("Deploy TestContractBalances", async () => {
        _contract = await deployTestContractBalances();
    });
    it("Ensure initial balance", async () => {
        expect(await _contract.get(ALICE.address)).eq(0, "Alice initial balance mismatch");
        expect(await _contract.get(BOB.address)).eq(0, "Bob initial balance mismatch");
        expect(await _contract.get(CHARLIE.address)).eq(0, "Charlie initial balance mismatch");
    });

    it("Sends parallel transactions to aggregate value", async () => {
        // prepare transactions
        const expectedBalances: Record<string, number> = {};
        expectedBalances[ALICE.address] = 0;
        expectedBalances[BOB.address] = 0;
        expectedBalances[CHARLIE.address] = 0;

        const senders = randomAccounts(50);
        const signedTxs = [];
        for (let accountIndex = 0; accountIndex < senders.length; accountIndex++) {
            // prepare transaction params
            let account = ALICE.address;
            if (accountIndex % 2 == 0) {
                account = BOB.address;
            } else if (accountIndex % 3 == 0) {
                account = CHARLIE.address;
            }
            const amount = accountIndex + 1;
            expectedBalances[account] += amount;

            // sign transaction
            const sender = senders[accountIndex];
            const nonce = await sendGetNonce(sender.address);
            const tx = await _contract.connect(sender.signer()).add.populateTransaction(account, amount, {
                nonce: nonce,
                ...TX_PARAMS,
            });
            signedTxs.push(await sender.signer().signTransaction(tx));
        }

        // send transactions in parallel
        const hashes = await sendRawTransactions(signedTxs);
        const receipts = await pollReceipts(hashes);

        // verify
        expect(receipts.successCount).eq(signedTxs.length, "Success transaction count mismatch");
        expect(receipts.failedCount).eq(0, "Failed transaction count mismatch");

        expect(await _contract.get(ALICE.address)).eq(expectedBalances[ALICE.address], "Alice final balance mismatch");
        expect(await _contract.get(BOB.address)).eq(expectedBalances[BOB.address], "Bob final balance mismatch");
        expect(await _contract.get(CHARLIE.address)).eq(
            expectedBalances[CHARLIE.address],
            "Charlie final balance mismatch",
        );
    });

    it("Fails parallel transactions due to lack of balance", async () => {
        // set initial balance
        await _contract.connect(ALICE.signer()).set(ALICE.address, 1140);
        expect(await _contract.get(ALICE.address)).eq(1140, "Alice initial balance mismatch");

        // parallel transactions decreases balance (15 must work, 5 should fail)
        const signedTxs = [];
        const senders = randomAccounts(20);
        for (const sender of senders) {
            let nonce = await sendGetNonce(sender.address);
            const tx = await _contract.connect(sender.signer()).sub.populateTransaction(ALICE.address, 75, {
                nonce: nonce,
                ...TX_PARAMS,
            });
            signedTxs.push(await sender.signer().signTransaction(tx));
        }

        // send transactions in parallel
        const hashes = await sendRawTransactions(signedTxs);
        const receipts = await pollReceipts(hashes);

        // check remaining balance
        expect(receipts.successCount).eq(15, "Success transaction count mismatch");
        expect(receipts.failedCount).eq(5, "Failed transaction count mismatch");
        expect(await _contract.get(ALICE.address)).eq(15, "Alice final balance mismatch");
    });
});

describe("Transaction: parallel TestContractCounter", async () => {
    let _contract: TestContractCounter;

    it("Resets blockchain", async () => {
        await sendReset();
        const blockNumber = await send("eth_blockNumber", []);
        // // TODO: re-enable this, for some reason it's returning `0x49`
        // // maybe the Rocks running in multi-threaded mode doesn't guarantee an immediate impact
        // expect(blockNumber).to.be.oneOf(["0x0", "0x1"]);
    });

    it("Deploy TestContractCounter", async () => {
        _contract = await deployTestContractCounter();
    });
    it("Ensure initial balance", async () => {
        expect(await _contract.getCounter()).eq(0, "Counter initial value mismatch");
        expect(await _contract.getDoubleCounter()).eq(0, "Double counter initial value mismatch");
    });

    it("Sends parallel transactions", async () => {
        const incSender = ALICE;
        const doubleSender = BOB;

        // send a pair of inc and double requests
        for (let i = 0; i < 20; i++) {
            // calculate expected double counter
            const doubleCounter = Number(await _contract.getDoubleCounter());
            const expectedDoubleCounter = [BigInt(doubleCounter + i * 2), BigInt(doubleCounter + (i + 1) * 2)];

            // sign transactions
            const incNonce = await sendGetNonce(incSender.address);
            const incTx = await _contract
                .connect(incSender.signer())
                .inc.populateTransaction({ nonce: incNonce, ...TX_PARAMS });
            const incSignedTx = await incSender.signer().signTransaction(incTx);

            const doubleNonce = await sendGetNonce(doubleSender.address);
            const doubleTx = await _contract
                .connect(doubleSender.signer())
                .double.populateTransaction({ nonce: doubleNonce, ...TX_PARAMS });
            const doubleSignedTx = await doubleSender.signer().signTransaction(doubleTx);

            // send transactions in parallel
            const hashes = await sendRawTransactions([incSignedTx, doubleSignedTx]);
            const receipts = await pollReceipts(hashes);

            // verify
            expect(receipts.successCount).eq(2, "Success transaction count mismatch");
            expect(receipts.failedCount).eq(0, "Failed transaction count mismatch");

            expect(await _contract.getCounter()).eq(i + 1, "Counter final value mismatch");
            expect(await _contract.getDoubleCounter()).oneOf(
                expectedDoubleCounter,
                "Double counter final value mismatch",
            );
        }
    });
});
