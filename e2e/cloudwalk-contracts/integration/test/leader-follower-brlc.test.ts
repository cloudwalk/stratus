import { expect } from "chai";
import { exec, execSync } from "child_process";
import { TransactionReceipt, TransactionResponse } from "ethers";
import { ethers } from "hardhat";

import {
    GAS_LIMIT_OVERRIDE,
    brlcToken,
    configureBRLC,
    deployBRLC,
    deployer,
    sendWithRetry,
    setDeployer,
    updateProviderUrl,
    waitForFollowerToSyncWithLeader,
} from "./helpers/rpc";

describe("Leader & Follower BRLC integration test", function () {
    // Helper function to get balance using direct RPC call
    // This ensures we're hitting the correct provider after updateProviderUrl
    async function getBalanceViaRPC(tokenAddress: string, walletAddress: string): Promise<bigint> {
        const balanceData = await sendWithRetry("eth_call", [
            {
                to: tokenAddress,
                data: brlcToken.interface.encodeFunctionData("balanceOf", [walletAddress]),
            },
            "latest",
        ]);

        if (!balanceData || balanceData === null) {
            throw new Error(`eth_call returned null/undefined for wallet ${walletAddress}. Contract may not exist.`);
        }

        return BigInt(balanceData);
    }

    it("Validate node modes for leader and follower", async function () {
        // Check Stratus Leader node mode
        updateProviderUrl("stratus");
        const leaderMode = await sendWithRetry("stratus_state", []);
        expect(leaderMode.is_leader).to.equal(true);

        // Check Stratus Leader health
        const leaderHealth = await sendWithRetry("stratus_health", []);
        expect(leaderHealth).to.equal(true);

        // Check Stratus Follower node mode
        updateProviderUrl("stratus-follower");
        const followerMode = await sendWithRetry("stratus_state", []);
        expect(followerMode.is_leader).to.equal(false);

        // Check Stratus Follower health
        const followerHealth = await sendWithRetry("stratus_health", []);
        expect(followerHealth).to.equal(true);

        updateProviderUrl("stratus");
    });

    before(async function () {
        await setDeployer();
    });

    describe("Deploy and configure BRLC contract using transaction forwarding from follower to leader", function () {
        it("Validate deployer is minter", async function () {
            updateProviderUrl("stratus-follower");

            await deployBRLC();
            await configureBRLC();

            expect(await brlcToken.hasRole(await brlcToken.MINTER_ROLE(), deployer.address)).to.equal(true);

            updateProviderUrl("stratus");
        });
    });

    describe("Long duration transaction tests", function () {
        const parameters = [
            { name: "Few wallets, sufficient balance", wallets: 3, duration: 15, tps: 100, baseBalance: 2000 },
            { name: "Few wallets, insufficient balance", wallets: 2, duration: 15, tps: 100, baseBalance: 5 },
            { name: "Many wallets, sufficient balance", wallets: 20, duration: 15, tps: 100, baseBalance: 2000 },
        ];
        parameters.forEach((params, index) => {
            const wallets: any[] = [];
            it(`${params.name}: Prepare and mint BRLC to wallets`, async function () {
                this.timeout(params.wallets * 1000 + 10000);

                for (let i = 0; i < params.wallets; i++) {
                    const wallet = ethers.Wallet.createRandom().connect(ethers.provider);
                    wallets.push(wallet);
                }

                for (let i = 0; i < wallets.length; i++) {
                    const wallet = wallets[i];
                    let tx = await brlcToken.mint(wallet.address, params.baseBalance, { gasLimit: GAS_LIMIT_OVERRIDE });
                    await tx.wait();
                }
            });

            let txHashList: string[] = [];
            let txList: TransactionResponse[] = [];
            it(`${params.name}: Transfer BRLC between wallets at a configurable TPS`, async function () {
                this.timeout(params.duration * 1000 + 30000);

                const transactionInterval = 1000 / params.tps;

                let nonces = await Promise.all(
                    wallets.map((wallet) => sendWithRetry("eth_getTransactionCount", [wallet.address, "latest"])),
                );

                const startTime = Date.now();
                while (Date.now() - startTime < params.duration * 1000) {
                    let senderIndex;
                    let receiverIndex;
                    do {
                        senderIndex = Math.floor(Math.random() * wallets.length);
                        receiverIndex = Math.floor(Math.random() * wallets.length);
                    } while (senderIndex === receiverIndex);

                    let sender = wallets[senderIndex];
                    let receiver = wallets[receiverIndex];

                    const amount = Math.floor(Math.random() * 10) + 1;

                    try {
                        const tx = await brlcToken.connect(sender).transfer(receiver.address, amount, {
                            gasPrice: 0,
                            gasLimit: GAS_LIMIT_OVERRIDE,
                            type: 0,
                            nonce: nonces[senderIndex],
                        });
                        txList.push(tx);
                        txHashList.push(tx.hash);
                    } catch (error) {
                        console.log(error);
                    }

                    nonces[senderIndex]++;

                    await new Promise((resolve) => setTimeout(resolve, transactionInterval));
                }

                // Wait for all transactions to be mined
                console.log(`          ✔ Waiting for ${txList.length} transactions to be mined...`);
                const receipts = await Promise.all(txList.map((tx) => tx.wait()));
                console.log(`          ✔ All ${receipts.length} transactions have been mined`);
            });

            it(`${params.name}: Validate transaction mined delay between Stratus Leader & Follower`, async function () {
                // Get Stratus Leader timestamps
                updateProviderUrl("stratus");
                const leaderTimestamps = await Promise.all(
                    txHashList.map(async (txHash) => {
                        const receipt = await sendWithRetry("eth_getTransactionReceipt", [txHash]);
                        const block = await sendWithRetry("eth_getBlockByNumber", [receipt.blockNumber, false]);
                        return parseInt(block.timestamp, 16);
                    }),
                );

                // Get Stratus Follower timestamps
                updateProviderUrl("stratus-follower");
                const followerTimestamps = await Promise.all(
                    txHashList.map(async (txHash) => {
                        const receipt = await sendWithRetry("eth_getTransactionReceipt", [txHash], 20);
                        const block = await sendWithRetry("eth_getBlockByNumber", [receipt.blockNumber, false]);
                        return parseInt(block.timestamp, 16);
                    }),
                );

                // Total time it took for Stratus Leader to process all the blocks containing transactions
                const leaderProcessingTime = leaderTimestamps[leaderTimestamps.length - 1] - leaderTimestamps[0];

                // Total time it took for Stratus Follower to process all the blocks containing transactions
                const followerProcessingTime =
                    followerTimestamps[followerTimestamps.length - 1] - followerTimestamps[0];

                console.log(`          ✔ Number of transactions sent: ${txHashList.length}`);
                console.log(
                    `          ✔ Stratus Leader processing time: ${leaderProcessingTime}s | Stratus Follower processing time: ${followerProcessingTime}s`,
                );
            });

            it(`${params.name}: Validate all transactions were imported from Stratus Leader to Follower`, async function () {
                // Get Stratus Leader transaction receipts
                updateProviderUrl("stratus");
                const leaderReceipts = await Promise.all(
                    txHashList.map(async (txHash) => {
                        const receipt = await sendWithRetry("eth_getTransactionReceipt", [txHash]);
                        return receipt;
                    }),
                );

                // Get Stratus Follower transaction receipts
                updateProviderUrl("stratus-follower");
                const followerReceipts = await Promise.all(
                    txHashList.map(async (txHash) => {
                        const receipt = await sendWithRetry("eth_getTransactionReceipt", [txHash]);
                        return receipt;
                    }),
                );

                // Assert that all transactions were imported
                for (let i = 0; i < txHashList.length; i++) {
                    expect(leaderReceipts[i]).to.exist;
                    expect(followerReceipts[i]).to.exist;
                }
            });

            it(`${params.name}: Validate each transaction was imported into the same block between Stratus Leader and Follower`, async function () {
                // Get Stratus Leader block numbers
                updateProviderUrl("stratus");
                const leaderBlockNumbers = await Promise.all(
                    txHashList.map(async (txHash) => {
                        const receipt = await sendWithRetry("eth_getTransactionReceipt", [txHash]);
                        return receipt.blockNumber;
                    }),
                );

                // Get Stratus Follower block numbers
                updateProviderUrl("stratus-follower");
                const followerBlockNumbers = await Promise.all(
                    txHashList.map(async (txHash) => {
                        const receipt = await sendWithRetry("eth_getTransactionReceipt", [txHash], 20);
                        return receipt.blockNumber;
                    }),
                );

                // Assert that each transaction fell into the same block between Stratus Leader and Follower
                for (let i = 0; i < txHashList.length; i++) {
                    expect(
                        leaderBlockNumbers[i],
                        `Transaction ${txHashList[i]} did not fall into the same block between Stratus Leader and Follower`,
                    ).to.equal(followerBlockNumbers[i]);
                }
            });

            it(`${params.name}: Validate that each transaction is in its corresponding block`, async function () {
                updateProviderUrl("stratus-follower");
                for await (const txHash of txHashList) {
                    const receipt: TransactionReceipt = await sendWithRetry("eth_getTransactionReceipt", [txHash], 20);
                    const block = await sendWithRetry("eth_getBlockByNumber", [receipt.blockNumber, true], 20);
                    expect(block).to.exist;

                    const transaction = block.transactions.find((tx: TransactionResponse) => tx.hash === txHash);
                    expect(transaction).to.exist;
                    expect(transaction!.blockNumber).to.equal(receipt.blockNumber);
                    expect(transaction!.blockHash).to.equal(receipt.blockHash);
                    for (const log of receipt!.logs) {
                        expect(log.blockNumber).to.equal(receipt.blockNumber);
                        expect(log.blockHash).to.equal(receipt.blockHash);
                    }
                    return receipt.blockNumber;
                }
            });

            it(`${params.name}: Validate balances between Stratus Leader and Follower`, async function () {
                for (let i = 0; i < wallets.length; i++) {
                    // Get Stratus Leader balance
                    updateProviderUrl("stratus");
                    const leaderBalance = await getBalanceViaRPC(await brlcToken.getAddress(), wallets[i].address);

                    // Get Stratus Follower balance
                    updateProviderUrl("stratus-follower");
                    const followerBalance = await getBalanceViaRPC(await brlcToken.getAddress(), wallets[i].address);

                    // Assert that the balances are equal
                    expect(leaderBalance).to.equal(
                        followerBalance,
                        `Wallet ${wallets[i].address} balances are not equal between Stratus Leader and Follower`,
                    );
                }
                updateProviderUrl("stratus");
            });
        });
    });

    describe("Follower recovery test", function () {
        const testWallets: any[] = [];
        let initialBalances: bigint[] = [];

        it("Setup test wallets with BRLC tokens", async function () {
            this.timeout(20000);

            // Make sure we're on leader
            updateProviderUrl("stratus");

            // Create 3 test wallets
            for (let i = 0; i < 3; i++) {
                const wallet = ethers.Wallet.createRandom().connect(ethers.provider);
                testWallets.push(wallet);

                // Mint 1000 BRLC to each wallet
                const mintAmount = 1000;
                const mintTx = await brlcToken.mint(wallet.address, mintAmount, { gasLimit: GAS_LIMIT_OVERRIDE });
                await mintTx.wait();

                const balance = await brlcToken.balanceOf(wallet.address);
                initialBalances.push(balance);
                expect(balance).to.equal(BigInt(mintAmount));
            }

            // Verify follower has the same balances
            updateProviderUrl("stratus-follower");
            const tokenAddress = await brlcToken.getAddress();
            for (let i = 0; i < testWallets.length; i++) {
                const followerBalance = await getBalanceViaRPC(tokenAddress, testWallets[i].address);
                expect(followerBalance).to.equal(initialBalances[i]);
            }

            updateProviderUrl("stratus");
        });

        it("Kill follower, send transactions to leader, restart follower and verify sync", async function () {
            this.timeout(60000);

            console.log("          ✔ Killing follower node...");

            // Kill the follower node
            try {
                execSync("killport 3001 -s sigterm", { stdio: "pipe" });
            } catch (error) {
                // Process might not exist, that's okay
            }

            // Wait a bit to ensure follower is down
            await new Promise((resolve) => setTimeout(resolve, 2000));

            // Verify follower is down by trying to connect
            let followerIsDown = false;
            try {
                updateProviderUrl("stratus-follower");
                await sendWithRetry("stratus_health", [], 1, 100);
            } catch (error) {
                followerIsDown = true;
            }
            expect(followerIsDown).to.equal(true, "Follower should be down");

            console.log("          ✔ Sending transactions to leader while follower is down...");

            // Switch to leader and send some transactions
            updateProviderUrl("stratus");

            const txHashes: string[] = [];
            const transferAmount = 50;

            // Send transfers between wallets (5 transactions)
            for (let i = 0; i < 5; i++) {
                const senderIndex = i % testWallets.length;
                const receiverIndex = (i + 1) % testWallets.length;

                const sender = testWallets[senderIndex];
                const receiver = testWallets[receiverIndex];

                const tx = await brlcToken.connect(sender).transfer(receiver.address, transferAmount, {
                    gasPrice: 0,
                    gasLimit: GAS_LIMIT_OVERRIDE,
                    type: 0,
                });

                txHashes.push(tx.hash);
                await tx.wait();
            }

            console.log(`          ✔ Sent ${txHashes.length} transactions to leader`);

            // Get final balances from leader
            const leaderBalances: bigint[] = [];
            for (const wallet of testWallets) {
                const balance = await brlcToken.balanceOf(wallet.address);
                leaderBalances.push(balance);
            }

            console.log("          ✔ Restarting follower node...");

            // Restart the follower with block changes replication enabled
            // Use exec instead of execSync since the follower runs in background
            exec(`just e2e-follower brlc true`, {
                shell: "/bin/bash",
            });

            // Wait for follower to be healthy
            console.log("          ✔ Waiting for follower to become healthy...");
            let followerHealthy = false;
            for (let i = 0; i < 30; i++) {
                try {
                    updateProviderUrl("stratus-follower");
                    const health = await sendWithRetry("stratus_health", [], 1, 100);
                    if (health === true) {
                        followerHealthy = true;
                        break;
                    }
                } catch (error) {
                    // Keep trying
                }
                await new Promise((resolve) => setTimeout(resolve, 1000));
            }
            expect(followerHealthy).to.equal(true, "Follower should become healthy");

            console.log("          ✔ Waiting for follower to sync with leader...");

            // Wait for follower to catch up
            const syncResult = await waitForFollowerToSyncWithLeader();
            console.log(`          ✔ Follower synced at block ${syncResult.followerBlock}`);

            // Verify all transactions are present in follower
            updateProviderUrl("stratus-follower");
            for (const txHash of txHashes) {
                const receipt = await sendWithRetry("eth_getTransactionReceipt", [txHash], 10, 2000);
                expect(receipt).to.exist;
                expect(receipt.status).to.equal("0x1");
            }

            console.log("          ✔ All transactions found in follower");

            // Verify balances match between leader and follower
            const tokenAddress = await brlcToken.getAddress();
            for (let i = 0; i < testWallets.length; i++) {
                const followerBalance = await getBalanceViaRPC(tokenAddress, testWallets[i].address);
                expect(followerBalance).to.equal(
                    leaderBalances[i],
                    `Wallet ${i} balance mismatch between leader and follower after recovery`,
                );
            }

            console.log("          ✔ All balances match after follower recovery");

            // Test a read operation (balanceOf) works correctly
            const testReadWallet = testWallets[0];
            const readBalance = await getBalanceViaRPC(tokenAddress, testReadWallet.address);
            expect(readBalance).to.be.a("bigint");
            expect(readBalance).to.equal(leaderBalances[0]);

            console.log("          ✔ Read operations (balanceOf) work correctly on recovered follower");

            updateProviderUrl("stratus");
        });
    });
});
