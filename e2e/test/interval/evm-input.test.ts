import { expect } from "chai";
import { ContractTransactionReceipt, Result } from "ethers";

import { deployTestEvmInput, sendReset } from "../helpers/rpc";

// This test needs to be ran with stratus on a very fast block production rate (around 10ms is fast enough)
describe("Evm Input", function () {
    it("should not be executed in one block but added to another", async function () {
        this.timeout(150000);

        console.log("Sending reset");
        await sendReset();
        console.log("Reset sent");
        const contract = await deployTestEvmInput();
        console.log("Contract deployed");
        await contract.waitForDeployment();
        console.log("Contract deployed");
        let tx = await contract.heavyComputation();
        console.log("Heavy computation sent");
        let receipt = (await tx.wait()) as ContractTransactionReceipt;
        console.log("Heavy computation receipt");
        const event = receipt.logs[0];
        console.log("Event parsed");
        const logs = contract.interface.parseLog({
            topics: event.topics,
            data: event.data,
        })?.args as Result;
        console.log("Event parsed");
        expect(receipt.blockNumber).to.eq(logs.blockNumber);
        expect(logs.blockNumber).gte(1000);
        expect(logs.isSlow).to.be.false;
        console.log("Get executions");
        expect(await contract.getExecutions()).to.eq(1);
        console.log("Get executions done");
    });
});
