import { expect } from "chai";
import { keccak256, TransactionReceipt } from "ethers";

import { ALICE } from "../../helpers/account";
import { BlockMode, currentBlockMode } from "../../helpers/network";
import {
    deployTestContractBalances,
    ETHERJS,
    prepareSignedTx,
    sendEvmMine,
    sendRawTransaction,
} from "../../helpers/rpc";

describe("Known issues for the external block mining mode. The 'eth_sendRawTransaction' API call", async () => {
    before(async () => {
        expect(currentBlockMode()).eq(
            BlockMode.External,
            "Wrong block mining mode is used"
        );
    });

    it("Returns an expected result when a contract transaction fails", async () => {
        const amount = 1;
        const contract = await deployTestContractBalances();
        await sendEvmMine();

        const signedTx = await prepareSignedTx({
            contract,
            account: ALICE,
            methodName: "sub",
            methodParameters: [ALICE.address, amount]
        });
        const expectedTxHash = keccak256(signedTx);

        const actualTxHash = await sendRawTransaction(signedTx);

        await sendEvmMine();
        const txReceipt: TransactionReceipt | null = await ETHERJS.getTransactionReceipt(expectedTxHash);

        expect(txReceipt).exist;
        expect(txReceipt?.status).eq(0); // The transaction really failed
        expect(actualTxHash).eq(expectedTxHash); // The transaction sending function returned the expected result
    });
});
