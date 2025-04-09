import { expect } from "chai";
import { ContractTransactionReceipt, Result } from "ethers";

import { deployTestEvmInput } from "../helpers/rpc";

// This test needs to be ran with stratus on a very fast block production rate (around 10ms is fast enough)
describe("Evm Input", function () {
    it("should not be executed in one block but added to another", async function () {
        const contract = await deployTestEvmInput();
        await contract.waitForDeployment();

        let tx = await contract.heavyComputation();

        let receipt = (await tx.wait()) as ContractTransactionReceipt;

        const event = receipt.logs[0];
        const logs = contract.interface.parseLog({
            topics: event.topics,
            data: event.data,
        })?.args as Result;
        expect(receipt.blockNumber).to.eq(logs.blockNumber);
        expect(logs.blockNumber).lte(5000);
        expect(logs.isSlow).to.be.true;
        expect(await contract.getExecutions()).to.eq(1);
    });
});
