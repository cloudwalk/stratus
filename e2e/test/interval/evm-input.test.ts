import { expect } from "chai";
import { ContractTransactionReceipt, Result } from "ethers";

import { deployTestEvmInput, sendReset } from "../helpers/rpc";

// This test needs to be ran with stratus on a very fast block production rate (around 10ms is fast enough)
describe("Evm Input", () => {
    describe("Transaction Execution", () => {
        it("should not be executed in one block but added to another", async () => {
            this.timeout(120000); // this test can take a while to run due to the heavy computation

            await sendReset();
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
            expect(logs.blockNumber).gte(1000);
            expect(logs.isSlow).to.be.false;
            expect(await contract.getExecutions()).to.eq(1);
        });
    });
});
