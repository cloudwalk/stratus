import { expect } from "chai";
import { ContractTransactionReceipt, Result } from "ethers";

import { changeMinerMode, deployTestEvmInput, sendEvmMine } from "../helpers/rpc";

// This test needs to be ran with stratus on a very fast block production rate (around 10ms is fast enough)
describe("Evm Input", function () {
    let contract: any;

    before(async function () {
        // Setup in external mode for contract deployment
        await changeMinerMode("external");

        // Deploy contract
        contract = await deployTestEvmInput();
        await sendEvmMine();
        await contract.waitForDeployment();
        console.log("Contract deployed");
    });

    beforeEach(async function () {
        // Change to interval mode with 10ms interval mining
        await changeMinerMode("10ms");
    });

    it("should not be executed in one block but added to another", async function () {
        // Run the heavy computation
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
        expect(logs.blockNumber).gte(1000);
        expect(logs.isSlow).to.be.false;
        expect(await contract.getExecutions()).to.eq(1);
    });
});
