import { expect } from "chai";
import { ContractTransactionReceipt, Result } from "ethers";

import { deployTestEvmInput } from "../helpers/rpc";

// This test needs to be ran with stratus on a very fast block production rate (around 10ms is fast enough)
describe("Evm Input", function () {
    it("should not be executed in one block but added to another", async function () {
        this.timeout(300000);

        console.log("Starting test");

        const contract = await deployTestEvmInput();
        await contract.waitForDeployment();
        console.log("Contract deployed");

        let tx = await contract.heavyComputation();
        console.log("Heavy computation sent");

        let receipt = (await tx.wait()) as ContractTransactionReceipt;
        console.log("Heavy computation receipt");

        const event = receipt.logs[0];
        const logs = contract.interface.parseLog({
            topics: event.topics,
            data: event.data,
        })?.args as Result;
        expect(receipt.blockNumber).to.eq(logs.blockNumber);
        expect(logs.blockNumber).gte(5000);
        expect(logs.isSlow).to.be.false;
        expect(await contract.getExecutions()).to.eq(1);
    });
});
