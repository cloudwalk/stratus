import { expect } from "chai";
import { ethers } from "hardhat";

import { ALICE, BOB } from "../helpers/account";
import {
    ZERO,
    deployTestContractBalances,
    send,
    sendGetNonce,
    sendRawTransaction,
    sendReset,
    toHex,
} from "../helpers/rpc";

describe("Development Endpoints", () => {
    beforeEach(async () => {
        await sendReset();
    });

    describe("setStorageAt", () => {
        it("should set storage value for an address", async () => {
            const slot = 0;
            const value = "0x0000000000000000000000000000000000000000000000000000000000000042";

            // Set storage value
            await send("hardhat_setStorageAt", [ALICE.address, toHex(slot), value]);

            // Verify storage value was set
            const result = await send("eth_getStorageAt", [ALICE.address, toHex(slot), "latest"]);
            expect(result).to.equal(value);
        });

        it("should overwrite existing storage values", async () => {
            const slot = 1;
            const initialValue = "0x0000000000000000000000000000000000000000000000000000000000000001";
            const newValue = "0x0000000000000000000000000000000000000000000000000000000000000002";

            // Set initial value
            await send("hardhat_setStorageAt", [ALICE.address, toHex(slot), initialValue]);

            // Verify initial value
            const result1 = await send("eth_getStorageAt", [ALICE.address, toHex(slot), "latest"]);
            expect(result1).to.equal(initialValue);

            // Overwrite with new value
            await send("hardhat_setStorageAt", [ALICE.address, toHex(slot), newValue]);

            // Verify new value
            const result2 = await send("eth_getStorageAt", [ALICE.address, toHex(slot), "latest"]);
            expect(result2).to.equal(newValue);
        });
    });

    describe("setNonce", () => {
        it("should set nonce for an address", async () => {
            const newNonce = 42;

            // Set nonce
            await send("hardhat_setNonce", [ALICE.address, toHex(newNonce)]);

            // Verify nonce was set
            const result = await send("eth_getTransactionCount", [ALICE.address, "latest"]);
            expect(parseInt(result, 16)).to.equal(newNonce);
        });

        it("should allow transactions with the exact nonce", async () => {
            const newNonce = 5;

            // Set nonce
            await send("hardhat_setNonce", [ALICE.address, toHex(newNonce)]);

            // Check if the nonce is set
            const result = await send("eth_getTransactionCount", [ALICE.address, "latest"]);
            expect(parseInt(result, 16)).to.equal(newNonce);

            // Send transaction with the exact nonce
            const signedTx = await ALICE.signWeiTransfer(BOB.address, 1, newNonce);
            const txHash = await sendRawTransaction(signedTx);

            // Check if the transaction was accepted
            const tx = await ethers.provider.getTransaction(txHash);
            expect(tx).to.not.be.null;

            // Check if the nonce increased to the expected value
            const newNonceAfterTx = await sendGetNonce(ALICE);
            expect(newNonceAfterTx).to.equal(newNonce + 1);
        });

        it("should reject transactions with incorrect nonce", async () => {
            const newNonce = 10;

            // Set nonce
            await send("hardhat_setNonce", [ALICE.address, toHex(newNonce)]);

            // Try to send transaction with incorrect nonce
            const incorrectNonce = newNonce - 1;
            const signedTx = await ALICE.signWeiTransfer(BOB.address, 1, incorrectNonce);

            // Expect transaction to be rejected
            try {
                await sendRawTransaction(signedTx);
                expect.fail("Transaction should have been rejected");
            } catch (error) {
                // Transaction was rejected as expected
                expect(error).to.exist;
            }
        });
    });

    describe("setBalance", () => {
        it("should set balance for an address", async () => {
            const newBalance = 456;

            // Set balance
            await send("hardhat_setBalance", [ALICE.address, toHex(newBalance)]);

            // Verify balance was set
            const result = await send("eth_getBalance", [ALICE.address, "latest"]);
            expect(result).to.equal(toHex(newBalance));
        });

        it("should allow balance transfer after setting initial balances", async () => {
            // Setup initial balances
            const aliceInitialBalance = 2000;
            const bobInitialBalance = 0;
            const transferAmount = 1000;

            // Set Alice and Bob's initial balances
            await send("hardhat_setBalance", [ALICE.address, toHex(aliceInitialBalance)]);
            await send("hardhat_setBalance", [BOB.address, toHex(bobInitialBalance)]);

            // Verify initial balances
            const actualAliceInitial = await send("eth_getBalance", [ALICE.address, "latest"]);
            const actualBobInitial = await send("eth_getBalance", [BOB.address, "latest"]);
            expect(BigInt(actualAliceInitial)).to.equal(BigInt(aliceInitialBalance));
            expect(BigInt(actualBobInitial)).to.equal(BigInt(bobInitialBalance));

            // Perform transfer from Alice to Bob
            const nonce = await sendGetNonce(ALICE);
            const signedTx = await ALICE.signWeiTransfer(BOB.address, transferAmount, nonce);
            await sendRawTransaction(signedTx);

            // Verify final balances
            const actualAliceFinal = await send("eth_getBalance", [ALICE.address, "latest"]);
            const actualBobFinal = await send("eth_getBalance", [BOB.address, "latest"]);

            // Bob should receive exactly the transfer amount
            expect(BigInt(actualBobFinal)).to.equal(BigInt(transferAmount));

            // Alice's balance should be reduced by transfer amount
            expect(BigInt(actualAliceFinal)).to.be.equal(BigInt(aliceInitialBalance - transferAmount));
        });

        it("should reject transactions that exceed the set balance", async () => {
            const newBalance = 500;
            const transferAmount = 1000;

            // Set balance
            await send("hardhat_setBalance", [ALICE.address, toHex(newBalance)]);

            // Try to send transaction that exceeds the balance
            const nonce = await sendGetNonce(ALICE);
            const signedTx = await ALICE.signWeiTransfer(BOB.address, transferAmount, nonce);

            // Expect transaction to be rejected
            try {
                await sendRawTransaction(signedTx);
                expect.fail("Transaction should have been rejected");
            } catch (error) {
                // Transaction was rejected as expected
                expect(error).to.exist;
            }
        });

        it("should set zero balance", async () => {
            // Set zero balance
            await send("hardhat_setBalance", [ALICE.address, ZERO]);

            // Verify balance was set to zero
            const result = await send("eth_getBalance", [ALICE.address, "latest"]);
            expect(result).to.equal(ZERO);

            // Try to send transaction with zero balance
            const nonce = await sendGetNonce(ALICE);
            const signedTx = await ALICE.signWeiTransfer(BOB.address, 1, nonce);

            // Expect transaction to be rejected
            try {
                await sendRawTransaction(signedTx);
                expect.fail("Transaction should have been rejected");
            } catch (error) {
                // Transaction was rejected as expected
                expect(error).to.exist;
            }
        });
    });

    describe("setCode", () => {
        it("should set code for an address", async () => {
            // Deploy a contract to get its bytecode
            const contract = await deployTestContractBalances();
            const contractAddress = await contract.getAddress();

            // Get the bytecode
            const bytecode = await ethers.provider.getCode(contractAddress);

            // Set the bytecode to a different address
            await send("hardhat_setCode", [BOB.address, bytecode]);

            // Verify code was set
            const result = await send("eth_getCode", [BOB.address, "latest"]);
            expect(result).to.equal(bytecode);
        });

        it("should replace existing code", async () => {
            // Deploy two different contracts
            const balancesContract = await deployTestContractBalances();
            const balancesAddress = await balancesContract.getAddress();
            const balancesBytecode = await ethers.provider.getCode(balancesAddress);

            const counterFactory = await ethers.getContractFactory("TestContractCounter");
            const counterContract = await counterFactory.connect(ALICE.signer()).deploy();
            const counterAddress = await counterContract.getAddress();
            const counterBytecode = await ethers.provider.getCode(counterAddress);

            // Replace the code of the balances contract with the counter contract code
            await send("hardhat_setCode", [balancesAddress, counterBytecode]);

            // Verify code was replaced
            const result = await send("eth_getCode", [balancesAddress, "latest"]);
            expect(result).to.equal(counterBytecode);
        });

        it("should set empty code for an address", async () => {
            // Set some non-empty code
            const contract = await deployTestContractBalances();
            const contractAddress = await contract.getAddress();
            const bytecode = await ethers.provider.getCode(contractAddress);

            await send("hardhat_setCode", [BOB.address, bytecode]);

            // Verify code was set
            const initialCode = await send("eth_getCode", [BOB.address, "latest"]);
            expect(initialCode).to.equal(bytecode);

            // Now set empty code
            const emptyBytecode = "0x";
            await send("hardhat_setCode", [BOB.address, emptyBytecode]);

            // Verify code was set to empty
            const result = await send("eth_getCode", [BOB.address, "latest"]);
            expect(result).to.equal(emptyBytecode);
        });
    });
});
