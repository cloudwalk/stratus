import { expect } from "chai";

import { ALICE, CHARLIE } from "../helpers/account";
import { BlockMode, currentBlockMode } from "../helpers/network";
import {
    TX_PARAMS,
    prepareSignedTx,
    send,
    sendClearCache,
    sendEvmMine,
    sendRawTransaction,
    sendReset,
    toPaddedHex,
} from "../helpers/rpc";

describe("Storage Merge Bug Regression", () => {
    before(() => {
        expect(currentBlockMode()).eq(BlockMode.External, "Wrong block mining mode is used");
    });

    it("Resets blockchain", async () => {
        await sendReset();
        const blockNumber = await send("eth_blockNumber", []);
        expect(blockNumber).to.be.oneOf(["0x0", "0x1"]);
    });

    it("Contract modified in the same block that it was deployed saves correctly", async () => {
        await sendReset();

        const { ethers } = await import("hardhat");
        console.log("Deploying contract");

        // Deploy contract
        const TestRWOperationsFactory = await ethers.getContractFactory("TestRWOperations");
        const contract = await TestRWOperationsFactory.connect(CHARLIE.signer()).deploy(TX_PARAMS);
        const contractAddress = await contract.getAddress();

        // Transfer native coin from ALICE to the contract address
        const transferTx = await ALICE.signer().sendTransaction({
            to: contractAddress,
            value: ethers.parseEther("1.0"),
            ...TX_PARAMS,
        });
        console.log("Transfer from ALICE to contract:", transferTx.hash);

        await sendEvmMine();
        await sendEvmMine();
        await sendEvmMine();
        await sendEvmMine();
        await sendClearCache();

        await contract.waitForDeployment();

        const contractCode = await send("eth_getCode", [contractAddress, "latest"]);
        console.log("Contract code length:", contractCode, contractAddress);
        expect(contractCode).to.not.equal("0x", "Contract code should not be empty");
        expect(contractCode.length).to.be.greaterThan(2, "Contract should have bytecode");
    });

    it("TX1 writes slot, TX2 reads but doesn't write - value should persist after mining", async () => {
        const { ethers } = await import("hardhat");
        console.log("Deploying contract");

        // Deploy contract
        const TestRWOperationsFactory = await ethers.getContractFactory("TestRWOperations");
        const contract = await TestRWOperationsFactory.connect(CHARLIE.signer()).deploy(TX_PARAMS);
        await sendEvmMine();
        await contract.waitForDeployment();
        const contractAddress = await contract.getAddress();

        console.log("Contract deployed at:", contractAddress);

        // Get initial nonce for CHARLIE
        let charlieNonce = Number(await send("eth_getTransactionCount", [CHARLIE.address, "latest"]));
        console.log("Initial nonce - CHARLIE:", charlieNonce);

        // TX1: Set value to 42 (modifies storage slot)
        const tx1 = await prepareSignedTx({
            contract,
            account: CHARLIE,
            methodName: "setValue",
            methodParameters: [42],
            custom_nonce: charlieNonce++,
        });
        const tx1Hash = await sendRawTransaction(tx1);
        console.log("TX1 (setValue) sent:", tx1Hash);

        // TX2: Read value (loads it into EVM) but modifies a different slot
        const tx2 = await prepareSignedTx({
            contract,
            account: CHARLIE,
            methodName: "readAndModifyOther",
            methodParameters: [999],
            custom_nonce: charlieNonce++,
        });
        const tx2Hash = await sendRawTransaction(tx2);
        console.log("TX2 (readAndModifyOther) sent:", tx2Hash);

        // Mine the block containing both transactions
        await sendEvmMine();
        await sendEvmMine();
        await sendEvmMine();
        await sendEvmMine();

        await sendClearCache();

        console.log("Block mined and cache cleared");

        // Verify TX1 executed successfully
        const receipt1 = await send("eth_getTransactionReceipt", [tx1Hash]);
        expect(receipt1.status).to.equal("0x1", "TX1 should succeed");

        // Verify TX2 executed successfully
        const receipt2 = await send("eth_getTransactionReceipt", [tx2Hash]);
        expect(receipt2.status).to.equal("0x1", "TX2 should succeed");

        // Read the value using eth_getStorageAt
        // Slot 0 is where "value" is stored
        const slot0 = toPaddedHex(0, 32);
        const storedValue = await send("eth_getStorageAt", [contractAddress, slot0, "latest"]);
        console.log("Stored value at slot 0:", storedValue);

        // The bug would cause this to be 0x0 instead of 0x2a (42)
        // because the merge() function would overwrite TX1's modification
        expect(storedValue).to.equal(toPaddedHex(42, 32), "Value should be 42 (set by TX1)");

        // Also verify via contract call
        const valueFromContract = await contract.value();
        expect(valueFromContract).to.equal(42n, "Value read from contract should be 42");
    });
});
