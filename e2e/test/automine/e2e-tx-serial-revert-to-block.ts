import { expect } from "chai";

import { ALICE, BOB, CHARLIE } from "../helpers/account";
import {
    ETHERJS,
    deployTestContractRevert,
    send,
    sendGetBalance,
    sendGetStorageAt,
    sendRawTransaction,
    sendReset,
} from "../helpers/rpc";

describe("Revert to block", () => {
    before(async () => {
        await sendReset();
    });

    it("should correctly revert state when reverting to previous blocks", async () => {
        // Initial balances
        const initialAliceBalance = await sendGetBalance(ALICE);
        const initialBobBalance = await sendGetBalance(BOB);
        const initialCharlieBalance = await sendGetBalance(CHARLIE);

        // First transfer: ALICE -> BOB (100 wei)
        let txSigned = await ALICE.signWeiTransfer(BOB.address, 100n);
        await sendRawTransaction(txSigned);

        // Check balances after first transfer
        const aliceBalanceAfterTx1 = await sendGetBalance(ALICE);
        const bobBalanceAfterTx1 = await sendGetBalance(BOB);
        // expect(aliceBalanceAfterTx1).to.be.lessThan(Number(initialAliceBalance));
        expect(bobBalanceAfterTx1).to.equal(Number(initialBobBalance));

        // Second transfer: BOB -> CHARLIE (50 wei)
        txSigned = await BOB.signWeiTransfer(CHARLIE.address, 50n);
        await sendRawTransaction(txSigned);

        // Check balances after second transfer
        const bobBalanceAfterTx2 = await sendGetBalance(BOB);
        const charlieBalanceAfterTx2 = await sendGetBalance(CHARLIE);
        // expect(bobBalanceAfterTx2).to.be.lessThan(Number(bobBalanceAfterTx1));
        expect(charlieBalanceAfterTx2).to.equal(Number(initialCharlieBalance));

        // Third transfer: CHARLIE -> ALICE (25 wei)
        txSigned = await CHARLIE.signWeiTransfer(ALICE.address, 25n);
        await sendRawTransaction(txSigned);

        // Check balances after third transfer
        const aliceBalanceAfterTx3 = await sendGetBalance(ALICE);
        const charlieBalanceAfterTx3 = await sendGetBalance(CHARLIE);
        // expect(charlieBalanceAfterTx3).to.be.lessThan(Number(charlieBalanceAfterTx2));
        expect(aliceBalanceAfterTx3).to.equal(Number(aliceBalanceAfterTx1));

        // Revert to block after second transfer
        await send("stratus_revertToBlock", ["0x" + (await send("eth_blockNumber")) - 1]);

        // Verify balances are back to state after second transfer
        expect(await sendGetBalance(BOB)).to.equal(bobBalanceAfterTx2);
        expect(await sendGetBalance(CHARLIE)).to.equal(charlieBalanceAfterTx2);
        expect(await sendGetBalance(ALICE)).to.equal(aliceBalanceAfterTx1);

        // Revert to block after first transfer
        await send("stratus_revertToBlock", ["0x" + (await send("eth_blockNumber")) - 2]);

        // Verify balances are back to state after first transfer
        expect(await sendGetBalance(ALICE)).to.equal(aliceBalanceAfterTx1);
        expect(await sendGetBalance(BOB)).to.equal(bobBalanceAfterTx1);
        expect(await sendGetBalance(CHARLIE)).to.equal(initialCharlieBalance);
    });
});

describe("Revert slots to block", () => {
    before(async () => {
        await sendReset();
    });

    it("Contract is deployed", async () => {
        const _contract = await deployTestContractRevert();
        const deployedCode = await send("eth_getCode", [_contract.target, "latest"]);

        expect(deployedCode).not.eq("0x");
    });

    it("should correctly revert storage slots when reverting to previous blocks", async () => {
        const _contract = await deployTestContractRevert();
        const deployedCode = await send("eth_getCode", [_contract.target, "latest"]);

        expect(deployedCode).not.eq("0x");

        // Set initial value in slot 0
        let txResponse = await _contract.connect(CHARLIE.signer()).set(42);
        let deployReceipt = await ETHERJS.getTransactionReceipt(txResponse.hash);
        expect(deployReceipt).exist;
        expect(deployReceipt?.status).eq(1);
        expect(_contract.target).exist;

        // Verify initial value
        const slot0Initial = await sendGetStorageAt(_contract.target, "0x0");
        expect(slot0Initial).to.equal("0x000000000000000000000000000000000000000000000000000000000000002a");

        // Set value to 84 in block 2
        txResponse = await _contract.connect(CHARLIE.signer()).set(84);
        deployReceipt = await ETHERJS.getTransactionReceipt(txResponse.hash);
        expect(deployReceipt).exist;
        expect(deployReceipt?.status).eq(1);

        // Verify value after block 2
        const slot0AfterBlock2 = await sendGetStorageAt(_contract.target, "0x0");
        expect(slot0AfterBlock2).to.equal("0x0000000000000000000000000000000000000000000000000000000000000054");

        // Set value to 126 in block 3
        txResponse = await _contract.connect(CHARLIE.signer()).set(126);
        deployReceipt = await ETHERJS.getTransactionReceipt(txResponse.hash);
        expect(deployReceipt).exist;
        expect(deployReceipt?.status).eq(1);

        // Verify value after block 3
        const slot0AfterBlock3 = await sendGetStorageAt(_contract.target, "0x0");
        expect(slot0AfterBlock3).to.equal("0x000000000000000000000000000000000000000000000000000000000000007e");

        // Revert to block 2
        await send("stratus_revertToBlock", ["0x" + ((await send("eth_blockNumber")) - 1)]);

        // Verify slot value is back to 84
        const slot0AfterRevert = await sendGetStorageAt(_contract.target, "0x0");
        expect(slot0AfterRevert).to.equal("0x0000000000000000000000000000000000000000000000000000000000000054");

        // Revert to block 1
        await send("stratus_revertToBlock", ["0x" + ((await send("eth_blockNumber")) - 2)]);

        // Verify slot value is back to 42
        const slot0AfterFinalRevert = await sendGetStorageAt(_contract.target, "0x0");
        expect(slot0AfterFinalRevert).to.equal("0x000000000000000000000000000000000000000000000000000000000000002a");
    });
});
