import { expect } from "chai";

import { TestContractBalances, TestContractCounter } from "../typechain-types";
import { ALICE, BOB, CHARLIE, randomAccounts } from "./helpers/account";
import {
    CHAIN_ID,
    GAS as GAS_LIMIT,
    TX_PARAMS,
    deployTestContractBalances,
    deployTestContractCounter,
    sendGetNonce,
    sendRawTransactions,
    sendReset,
} from "./helpers/rpc";

describe("Transaction: parallel TestContractBalances", async () => {
    var _contract: TestContractBalances;

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

        // send transactions in parallel
        await sendRawTransactions(signedTxs);

        // verify
        expect(await _contract.get(ALICE.address)).eq(expectedBalances[ALICE.address]);
        expect(await _contract.get(BOB.address)).eq(expectedBalances[BOB.address]);
        expect(await _contract.get(CHARLIE.address)).eq(expectedBalances[CHARLIE.address]);
    });

    it("Sends parallel transactions that should have one success and one fail due to lack of balance", async () => {
        expect(await _contract.get(BOB.address)).eq(625);

        const signedTxsSub = [];

        for (let i = 0; i < 2; i++) {
            const nonce = await sendGetNonce(ALICE.address);
            const tx = await _contract.connect(ALICE.signer()).sub.populateTransaction(BOB.address, 600, {
                nonce: nonce,
                ...TX_PARAMS,
            });

            signedTxsSub.push(await ALICE.signer().signTransaction(tx));
        }

        const result = await sendRawTransactions(signedTxsSub);

        // only one transaction should be successful
        expect(await _contract.get(BOB.address)).eq(25);
        expect(result[1]).eq(undefined);
    });
});

describe("Transaction: parallel TestContractCounter", async () => {
    var _contract: TestContractCounter;

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

        // send pair of inc and double requests
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
            await sendRawTransactions([incSignedTx, doubleSignedTx]);

            // verify
            expect(await _contract.getCounter()).eq(i + 1);
            expect(await _contract.getDoubleCounter()).oneOf(expectedDoubleCounter);
        }
    });
});
