import { Account, ALICE, BOB, randomAccounts } from "../helpers/account";
import { deployTestContractDenseStorage, sendEvmMine, sendRawTransactions, } from "../helpers/rpc";
import {
    AccountRecord,
    applyRecordChange,
    compareRecords,
    defineRecordChange,
    initialRecord,
    inverseRecordChange,
    prepareSignedTxOfRecordChange
} from "../helpers/contract-dense-storage";

describe("Transaction: parallel for the 'TestContractDenseStorage' contract", async () => {
    it("Parallel transactions execute properly when no reverts are expected", async () => {
        const contract = await deployTestContractDenseStorage();
        await sendEvmMine();

        const expectedRecords: Record<string, AccountRecord> = {};
        expectedRecords[ALICE.address] = { ...initialRecord };
        expectedRecords[BOB.address] = { ...initialRecord };

        // Set initial records
        await contract.set(ALICE.address, expectedRecords[ALICE.address]);
        await sendEvmMine();
        await contract.set(BOB.address, expectedRecords[BOB.address]);
        await sendEvmMine();


        // Prepare a pair of transactions: one for Alice and another for Bob
        const txCountPerUser = 32;
        const senders: Account[] = randomAccounts(txCountPerUser * 2);
        const signedTxs: string[] = [];
        for (let txIndexPerUser = 0; txIndexPerUser < txCountPerUser; ++txIndexPerUser) {
            const recordChange1 = defineRecordChange(txIndexPerUser);
            const recordChange2 = inverseRecordChange(recordChange1);
            applyRecordChange(expectedRecords[ALICE.address], recordChange1);
            applyRecordChange(expectedRecords[BOB.address], recordChange2);

            const sender1 = senders[txIndexPerUser * 2];
            const sender2 = senders[txIndexPerUser * 2 + 1];
            const signTx1: string = await prepareSignedTxOfRecordChange(contract, sender1, ALICE, recordChange1);
            const signTx2: string = await prepareSignedTxOfRecordChange(contract, sender2, BOB, recordChange2);

            signedTxs.push(signTx1);
            signedTxs.push(signTx2);
        }

        // Send transactions in parallel
        await sendRawTransactions(signedTxs);
        await sendEvmMine();

        // Verify
        compareRecords(await contract.get(ALICE.address), expectedRecords[ALICE.address]);
        compareRecords(await contract.get(BOB.address), expectedRecords[BOB.address]);
    });
});
