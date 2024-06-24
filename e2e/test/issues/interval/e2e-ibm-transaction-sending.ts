import { expect } from "chai";
import { TransactionReceipt, keccak256 } from "ethers";

import { ALICE } from "../../helpers/account";
import { checkTimeEnough } from "../../helpers/misc";
import { BlockMode, currentBlockMode, currentMiningIntervalInMs } from "../../helpers/network";
import {
    ETHERJS,
    HASH_ZERO,
    deployTestContractBalances,
    pollForTransaction,
    prepareSignedTx,
    sendRawTransaction,
} from "../../helpers/rpc";

describe("Known issues for the interval block mining mode. The 'eth_sendRawTransaction' API call", async () => {
    let blockIntervalInMs: number;

    before(async () => {
        expect(currentBlockMode()).eq(BlockMode.Interval, "Wrong block mining mode is used");
        blockIntervalInMs = currentMiningIntervalInMs() ?? 0;
        expect(blockIntervalInMs).gte(1000, "Block interval must be at least 1000 ms");
    });

    it("Returns an expected result when a contract transaction fails", async () => {
        const amount = 1;
        const contract = await deployTestContractBalances();
        const signedTx = await prepareSignedTx({
            contract,
            account: ALICE,
            methodName: "sub",
            methodParameters: [ALICE.address, amount],
        });
        const expectedTxHash = keccak256(signedTx);

        await pollForTransaction(contract.deploymentTransaction()?.hash ?? HASH_ZERO, {
            timeoutInMs: blockIntervalInMs * 1.1,
        });
        const txSendingTime = Date.now();
        const actualTxHash = await sendRawTransaction(signedTx);
        checkTimeEnough(txSendingTime, blockIntervalInMs);

        await pollForTransaction(expectedTxHash, { timeoutInMs: blockIntervalInMs * 1.1 });
        const txReceipt: TransactionReceipt | null = await ETHERJS.getTransactionReceipt(expectedTxHash);

        expect(txReceipt).exist;
        expect(txReceipt?.status).eq(0); // The transaction failed
        expect(actualTxHash).eq(expectedTxHash); // The transaction sending function returned the expected result
    });
});
