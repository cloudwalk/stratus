import { expect } from "chai";
import { TransactionReceipt } from "ethers";

import { ALICE } from "../../helpers/account";
import { BlockMode, currentBlockMode } from "../../helpers/network";
import { deployTestContractBalances, ETHERJS, send, toHex } from "../../helpers/rpc";

describe("Known issues for the auto block mining mode. The results of the 'eth_getLogs' API call are", async () => {
    before(async () => {
        expect(currentBlockMode()).eq(
            BlockMode.Automine,
            "Wrong block mining mode is used"
        );
    });

    it("Correct for next non-existent blocks after a contract transaction", async () => {
        const contract = await deployTestContractBalances();

        const txResponse = await contract.connect(ALICE.signer()).add(ALICE.address, 10);
        const txReceipt: TransactionReceipt | null = await ETHERJS.getTransactionReceipt(txResponse.hash);
        expect(txReceipt).exist;

        const txBlockNumber = txReceipt?.blockNumber ?? 0;

        expect(
            await send("eth_getLogs", [{ fromBlock: toHex(txBlockNumber), address: contract.target }])
        ).length(1);
        expect(
            await send("eth_getLogs", [{ fromBlock: toHex(txBlockNumber + 1), address: contract.target }])
        ).length(0);
        expect(
            await send("eth_getLogs", [{ fromBlock: toHex(txBlockNumber + 2), address: contract.target }])
        ).length(0);
    });
});
