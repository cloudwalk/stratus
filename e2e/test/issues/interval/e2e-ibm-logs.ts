import { expect } from "chai";

import { ALICE } from "../../helpers/account";
import { checkTimeEnough } from "../../helpers/misc";
import { BlockMode, currentBlockMode, currentMiningIntervalInMs } from "../../helpers/network";
import { HASH_ZERO, deployTestContractBalances, pollForTransaction, send, toHex } from "../../helpers/rpc";

describe("Known issues for the interval block mining mode. The 'eth_getLogs' API call", async () => {
    let blockIntervalInMs: number;

    before(async () => {
        expect(currentBlockMode()).eq(BlockMode.Interval, "Wrong block mining mode is used");
        blockIntervalInMs = currentMiningIntervalInMs() ?? 0;
        expect(blockIntervalInMs).gte(1000, "Block interval must be at least 1000 ms");
    });

    it("Returns an expected array for next non-existent blocks after a contract transaction", async () => {
        const contract = await deployTestContractBalances();
        await pollForTransaction(contract.deploymentTransaction()?.hash ?? HASH_ZERO, {
            timeoutInMs: blockIntervalInMs * 1.1,
        });

        const txSendingTime = Date.now();
        const txResponse = await contract.connect(ALICE.signer()).add(ALICE.address, 10);
        const txReceipt = await pollForTransaction(txResponse.hash, { timeoutInMs: blockIntervalInMs * 1.1 });

        const txBlockNumber = txReceipt.blockNumber;

        expect(await send("eth_getLogs", [{ fromBlock: toHex(txBlockNumber), address: contract.target }])).length(1);
        expect(await send("eth_getLogs", [{ fromBlock: toHex(txBlockNumber + 1), address: contract.target }])).length(
            0,
        );
        expect(await send("eth_getLogs", [{ fromBlock: toHex(txBlockNumber + 2), address: contract.target }])).length(
            0,
        );

        checkTimeEnough(txSendingTime, blockIntervalInMs * 1.7);
    });
});
