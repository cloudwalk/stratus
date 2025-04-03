import { expect } from "chai";
import { ContractTransactionReceipt, Result } from "ethers";

import { deployTestEvmInput, sendReset } from "../helpers/rpc";

// This test needs to be ran with stratus on a very fast block production rate (around 10ms is fast enough)
describe("Evm Input", function () {
    it("should not be executed in one block but added to another", async function () {
        this.timeout(180000); // this test can take a while to run due to the heavy computation

        console.log("Sending reset");
        await sendReset();
        console.log("Deploying contract");
        const contract = await deployTestEvmInput();
        await contract.waitForDeployment();
        console.log("Executing heavy computation");
        let tx = await contract.heavyComputation();
        console.log("Waiting for receipt");
        let receipt = (await tx.wait()) as ContractTransactionReceipt;
        console.log("Parsing logs");
        const event = receipt.logs[0];
        const logs = contract.interface.parseLog({
            topics: event.topics,
            data: event.data,
        })?.args as Result;
        console.log("Checking results");

        expect(receipt.blockNumber).to.eq(logs.blockNumber);
        expect(logs.blockNumber).gte(1000);
        expect(logs.isSlow).to.be.false;
        console.log("Checking executions");
        expect(await contract.getExecutions()).to.eq(1);
    });
});
