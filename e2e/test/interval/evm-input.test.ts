import { expect } from "chai";
import { ContractTransactionReceipt, Result } from "ethers";

import { deployTestEvmInput, sendAndGetFullResponse, sendEvmMine } from "../helpers/rpc";

// This test needs to be ran with stratus on a very fast block production rate (around 10ms is fast enough)
describe("Evm Input", function () {
    // We change miner mode to external when deploying the contract to avoid freezing the test
    it("should not be executed in one block but added to another", async function () {
        // Disable transactions to allow miner mode change
        const disableTransactionsResponse = await sendAndGetFullResponse("stratus_disableTransactions", []);
        expect(disableTransactionsResponse.data.result).to.equal(false);
        console.log("Transactions disabled");

        // Disable miner to allow mode change
        const disableMinerResponse = await sendAndGetFullResponse("stratus_disableMiner", []);
        expect(disableMinerResponse.data.result).to.equal(false);
        console.log("Miner disabled");

        // Change to external mode first
        const changeToExternalResponse = await sendAndGetFullResponse("stratus_changeMinerMode", ["external"]);
        expect(changeToExternalResponse.data.result).to.equal(true);
        console.log("Changed to external mode");

        // Enable transactions for deployment
        const enableTransactionsResponse = await sendAndGetFullResponse("stratus_enableTransactions", []);
        expect(enableTransactionsResponse.data.result).to.equal(true);
        console.log("Transactions enabled");

        // Enable miner for deployment
        const enableMinerResponse = await sendAndGetFullResponse("stratus_enableMiner", []);
        expect(enableMinerResponse.data.result).to.equal(true);
        console.log("Miner enabled");

        // Deploy contract
        const contract = await deployTestEvmInput();
        await sendEvmMine();
        await contract.waitForDeployment();
        console.log("Contract deployed");

        // Disable transactions again to change miner mode
        const disableTransactionsAgainResponse = await sendAndGetFullResponse("stratus_disableTransactions", []);
        expect(disableTransactionsAgainResponse.data.result).to.equal(false);
        console.log("Transactions disabled again");

        // Disable miner again
        const disableMinerAgainResponse = await sendAndGetFullResponse("stratus_disableMiner", []);
        expect(disableMinerAgainResponse.data.result).to.equal(false);
        console.log("Miner disabled again");

        // Change to interval mode with 10ms
        const changeToIntervalResponse = await sendAndGetFullResponse("stratus_changeMinerMode", ["10ms"]);
        expect(changeToIntervalResponse.data.result).to.equal(true);
        console.log("Changed to interval mode (10ms)");

        // Enable transactions for tests
        const enableTransactionsForTestsResponse = await sendAndGetFullResponse("stratus_enableTransactions", []);
        expect(enableTransactionsForTestsResponse.data.result).to.equal(true);
        console.log("Transactions enabled for tests");

        // Enable miner for tests
        const enableMinerForTestsResponse = await sendAndGetFullResponse("stratus_enableMiner", []);
        expect(enableMinerForTestsResponse.data.result).to.equal(true);
        console.log("Miner enabled for tests");
        
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
