import { ethers } from "hardhat";
import { expect } from "chai";
import {
    GAS_LIMIT_OVERRIDE,
    brlcToken,
    configureBRLC,
    deployBRLC,
    setDeployer, 
    send,
    deployer,
    updateProviderUrl
} from "./helpers/rpc";

describe("Relayer integration test", function () {
    before(async function () {
        await setDeployer();
    });

    describe("Deploy and configure BRLC contract", function () {
        it("Validate deployer is main minter", async function () {
            await deployBRLC();
            await configureBRLC();

            expect(deployer.address).to.equal(await brlcToken.mainMinter());
            expect(await brlcToken.isMinter(deployer.address)).to.be.true;
        });

    });
    describe("Transaction tests", function () {
        let alice = ethers.Wallet.createRandom().connect(ethers.provider);
        let bob = ethers.Wallet.createRandom().connect(ethers.provider);

        it("Validate mint BRLC to wallets", async function () {      
            expect(await await ethers.provider.getBalance(alice.address)).to.be.equal(0);
            expect(await await ethers.provider.getBalance(bob.address)).to.be.equal(0);

            expect(await brlcToken.mint(alice.address, 2000, { gasLimit: GAS_LIMIT_OVERRIDE })).to.have.changeTokenBalance(brlcToken, alice, 2000);
            expect(await brlcToken.mint(bob.address, 2000, { gasLimit: GAS_LIMIT_OVERRIDE })).to.have.changeTokenBalance(brlcToken, bob, 2000);

            expect(await brlcToken.balanceOf(alice.address)).to.equal(2000);
            expect(await brlcToken.balanceOf(bob.address)).to.equal(2000);
        });

        let txHashList: string[] = []
        it("Transfer BRLC between wallets at a configurable TPS", async function () {
            const testDuration = 10;
            this.timeout(testDuration * 1000 + 10000);

            const TPS = 5;
            const wallets = [alice, bob];
            const transactionInterval = 1000 / TPS;

            let nonces = await Promise.all(wallets.map(wallet => send("eth_getTransactionCount", [wallet.address, "latest"])));

            const startTime = Date.now();
            while (Date.now() - startTime < testDuration * 1000) {
                let senderIndex;
                let receiverIndex;
                do {
                    senderIndex = Math.floor(Math.random() * wallets.length);
                    receiverIndex = Math.floor(Math.random() * wallets.length);
                } while (senderIndex === receiverIndex);

                let sender = wallets[senderIndex];
                let receiver = wallets[receiverIndex];

                const amount = Math.floor(Math.random() * 10) + 1;

                const tx = await brlcToken.connect(sender).transfer(receiver.address, amount, { gasPrice: 0, gasLimit: GAS_LIMIT_OVERRIDE, type: 0, nonce: nonces[senderIndex] });
                txHashList.push(tx.hash);

                nonces[senderIndex]++;

                await new Promise(resolve => setTimeout(resolve, transactionInterval));
            }
            await new Promise(resolve => setTimeout(resolve, 5000));
        });

        it("Validate transaction mined delay between Stratus and Hardhat", async function () {
            // Get Stratus timestamps
            updateProviderUrl("stratus");
            const stratusTimestamps = await Promise.all(txHashList.map(async (txHash) => {
                const receipt = await send("eth_getTransactionReceipt", [txHash]);
                const block = await send("eth_getBlockByNumber", [receipt.blockNumber, false]);
                return parseInt(block.timestamp, 16);
            }));

            // Get Hardhat timestamps
            updateProviderUrl("hardhat");
            const hardhatTimestamps = await Promise.all(txHashList.map(async (txHash) => {
                const receipt = await send("eth_getTransactionReceipt", [txHash]);
                const block = await send("eth_getBlockByNumber", [receipt.blockNumber, false]);
                return parseInt(block.timestamp, 16);
            }));

            // Calculate the difference in timestamps(seconds) between Stratus and Hardhat
            const timestampDifferences = stratusTimestamps.map((stratusTimestamp, i) => {
                const difference = Math.abs(stratusTimestamp - hardhatTimestamps[i]);
                return difference;
            });

            // Calculate the average delay
            const averageDelay = timestampDifferences.reduce((a, b) => a + b, 0) / timestampDifferences.length;

            if (averageDelay > 1) {
                console.log(`WARN: Average delay is ${averageDelay.toFixed(2)}, which is above 1`);
            }
        });

        it("Validate balances between Stratus and Hardhat", async function () {
            // Get Stratus balances
            updateProviderUrl("stratus");
            const stratusAliceBalance = await brlcToken.balanceOf(alice.address);
            const stratusBobBalance = await brlcToken.balanceOf(bob.address);

            // Get Hardhat balances
            updateProviderUrl("hardhat");
            const hardhatAliceBalance = await brlcToken.balanceOf(alice.address);
            const hardhatBobBalance = await brlcToken.balanceOf(bob.address);

            // Assert that the balances are equal
            expect(stratusAliceBalance).to.equal(hardhatAliceBalance, "Alice balances are not equal between Stratus and Hardhat");
            expect(stratusBobBalance).to.equal(hardhatBobBalance, "Alice balances are not equal between Stratus and Hardhat");
        });

        it("Validate transactions were relayed from Stratus to Hardhat", async function () {
            // Get Stratus transaction receipts
            updateProviderUrl("stratus");
            const stratusReceipts = await Promise.all(txHashList.map(async (txHash) => {
                const receipt = await send("eth_getTransactionReceipt", [txHash]);
                return receipt;
            }));

            // Get Hardhat transaction receipts
            updateProviderUrl("hardhat");
            const hardhatReceipts = await Promise.all(txHashList.map(async (txHash) => {
                const receipt = await send("eth_getTransactionReceipt", [txHash]);
                return receipt;
            }));

            // Assert that all transactions were relayed
            for (let i = 0; i < txHashList.length; i++) {
                expect(stratusReceipts[i]).to.exist;
                expect(hardhatReceipts[i]).to.exist;
            }
        });
    });
});