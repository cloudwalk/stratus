import { expect } from "chai";

import { TestContractBalances, TestContractCounter } from "../../typechain-types";
import { ALICE, BOB, CHARLIE, randomAccounts } from "../helpers/account";
import {
    DEFAULT_TX_TIMEOUT_MS,
    deployTestContractBalances,
    deployTestContractCounter,
    getTransactionResponses,
    mineBlockIfNeeded,
    proveTx,
    proveTxs,
    sendGetNonce,
    sendRawTransactions,
    sendReset,
    TX_PARAMS,
    waitForTransactions
} from "../helpers/rpc";
import { keccak256 } from "ethers";
import { BlockMode, currentBlockMode, isStratus } from "../helpers/network";

describe("Transaction: parallel TestContractBalances", async () => {
    let _contract: TestContractBalances;

    it("Resets blockchain", async () => {
        await sendReset();
    });

    it("Deploy TestContractBalances", async () => {
        _contract = await deployTestContractBalances();
    });
    it("Ensure initial balance", async () => {
        expect(await _contract.get(ALICE.address)).eq(0);
        expect(await _contract.get(BOB.address)).eq(0);
        expect(await _contract.get(CHARLIE.address)).eq(0);
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

        // send transactions in parallel and wait for minting
        const actualTxHashes = await sendRawTransactions(signedTxs);
        // [POSSIBLE_BUG] Fails for Stratus in 'external' and '1s' block modes.
        // The failure reason: function 'getTransactionResponses()' cannot find the transactions by their hashes
        // whereas the transactions must be available even before minting.
        if (!isStratus || currentBlockMode() === BlockMode.Automine) {
            await getTransactionResponses(actualTxHashes);
        }
        await proveTxs(actualTxHashes);

        // compare transaction hashes
        const expectedTxHashes = signedTxs.map(signedTx => keccak256(signedTx));
        actualTxHashes.sort();
        expectedTxHashes.sort();
        expect(actualTxHashes).deep.eq(expectedTxHashes);

        // verify
        expect(await _contract.get(ALICE.address)).eq(expectedBalances[ALICE.address]);
        expect(await _contract.get(BOB.address)).eq(expectedBalances[BOB.address]);
        expect(await _contract.get(CHARLIE.address)).eq(expectedBalances[CHARLIE.address]);
    });

    it("Fails parallel transactions due to lack of balance", async () => {
        // set initial balance
        await proveTx(_contract.connect(ALICE.signer()).set(ALICE.address, 1140));
        expect(await _contract.get(ALICE.address)).eq(1140);

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
        let hashes = await sendRawTransactions(signedTxs);
        let failed = hashes.filter(x => x === undefined).length;

        // Check transaction receipts
        const allTxHashes = signedTxs.map(signedTx => keccak256(signedTx));

        // [POSSIBLE_BUG] If you put the 'mineBlockIfNeeded()' function call after
        // the `getTransactionResponses()` function one the test fails on Stratus in the `external` block mode.
        // The failure reason: function 'getTransactionResponses()' cannot find the transactions by their hashes
        // whereas the transactions must be available even before minting.
        // The test passes on Hardhat in all cases.
        await mineBlockIfNeeded();

        // [POSSIBLE_BUG] Without this 'if' statement the test fails on the next line after it when running on Stratus.
        // The failure reason: function 'getTransactionResponses()' cannot find the transactions by their hashes
        // whereas the transactions must be available even before minting.
        if (currentBlockMode() === BlockMode.Interval) {
            await new Promise((resolve) => setTimeout(resolve, DEFAULT_TX_TIMEOUT_MS));
        }
        await getTransactionResponses(allTxHashes);
        const txReceipts = await waitForTransactions(allTxHashes);
        const failedReceipts = txReceipts.filter(txReceipt => txReceipt.status !== 1).length;

        // Check transaction hashes
        const expectedRevertedTxHashes = txReceipts
            .filter(txReceipt => txReceipt.status !== 1)
            .map(txReceipt => txReceipt.hash);
        const expectedSuccessTxHashes = txReceipts
            .filter(txReceipt => txReceipt.status === 1)
            .map(txReceipt => txReceipt.hash);
        let expectedTxHashes: string[];
        if (currentBlockMode() === BlockMode.Automine) {
            expectedTxHashes = [...expectedSuccessTxHashes];
        } else {
            // [POSSIBLE_BUG] Different behaviour between Hardhat and Stratus
            if (isStratus) {
                expectedTxHashes = [...expectedSuccessTxHashes];
            } else {
                expectedTxHashes = [...expectedRevertedTxHashes, ...expectedSuccessTxHashes];
            }
        }
        const actualTxHashes = hashes.filter(hash => !!hash);
        actualTxHashes.sort();
        expectedTxHashes.sort();
        expect(actualTxHashes).deep.eq(expectedTxHashes);

        // check remaining balance
        expect(await _contract.get(ALICE.address)).eq(15);
        expect(failedReceipts).eq(5, "failed transactions");
        if (currentBlockMode() === BlockMode.Automine) {
            expect(failed).eq(5, "failed transactions");
        } else {
            if (isStratus) {
                expect(failed).eq(5, "failed transactions");
            } else {
                expect(failed).eq(0, "failed transactions");
            }
        }
    });
});

describe("Transaction: parallel TestContractCounter", async () => {
    let _contract: TestContractCounter;

    it("Resets blockchain", async () => {
        await sendReset();
    });

    it("Deploy TestContractCounter", async () => {
        _contract = await deployTestContractCounter();
    });
    it("Ensure initial balance", async () => {
        expect(await _contract.getCounter()).eq(0);
        expect(await _contract.getDoubleCounter()).eq(0);
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
            const signedTxs = [incSignedTx, doubleSignedTx];
            const txHashes = signedTxs.map(signedTx => keccak256(signedTx));
            await sendRawTransactions(signedTxs);
            await proveTxs(txHashes);

            // verify
            expect(await _contract.getCounter()).eq(i + 1);
            expect(await _contract.getDoubleCounter()).oneOf(expectedDoubleCounter);
        }
    });
});
