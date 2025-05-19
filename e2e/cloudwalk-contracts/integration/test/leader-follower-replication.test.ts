import { expect } from "chai";
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

describe("Leader & Follower replication integration test", function () {
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
                    expect(
                        await brlcToken.mint(wallet.address, params.baseBalance, { gasLimit: GAS_LIMIT_OVERRIDE }),
                    ).to.have.changeTokenBalance(brlcToken, wallet, params.baseBalance);
                }
            });

            let txHashList: string[] = [];
            it(`${params.name}: Transfer BRLC between wallets at a configurable TPS`, async function () {
                this.timeout(params.duration * 1000 + 10000);

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
                        txHashList.push(tx.hash);
                    } catch (error) {}

                    nonces[senderIndex]++;

                    await new Promise((resolve) => setTimeout(resolve, transactionInterval));
                }
            });

            it("Validate replication logs are identical between leader and follower", async function () {
                this.timeout(60000);

                // Wait for follower to sync with leader
                await waitForFollowerToSyncWithLeader();

                // Get the earliest block
                updateProviderUrl("stratus");
                const earliestBlock = await sendWithRetry("eth_getBlockByNumber", ["earliest", false]);
                const earliestBlockNumber = parseInt(earliestBlock.number, 16);

                // Get the latest block
                const latestBlock = await sendWithRetry("eth_getBlockByNumber", ["latest", false]);
                const latestBlockNumber = parseInt(latestBlock.number, 16);

                // Iterate through each block
                for (let blockNumber = earliestBlockNumber; blockNumber <= latestBlockNumber; blockNumber++) {
                    // Get replication log from leader
                    updateProviderUrl("stratus");
                    const leaderReplicationLog = await sendWithRetry("stratus_getReplicationLog", [blockNumber]);

                    // Get replication log from follower
                    updateProviderUrl("stratus-follower");
                    const followerReplicationLog = await sendWithRetry("stratus_getReplicationLog", [blockNumber]);

                    // Validate that both replication logs exist
                    expect(leaderReplicationLog, `Leader replication log for block ${blockNumber} is null`).to.not.be
                        .null;
                    expect(followerReplicationLog, `Follower replication log for block ${blockNumber} is null`).to.not
                        .be.null;

                    // Parse the block numbers from the response (they are hex strings)
                    const leaderBlockNumber = parseInt(leaderReplicationLog.block_number, 16);
                    const followerBlockNumber = parseInt(followerReplicationLog.block_number, 16);

                    // Validate that the block numbers in the response match the requested block number
                    expect(
                        leaderBlockNumber,
                        `Leader replication log block number (${leaderReplicationLog.block_number}) doesn't match requested block number (${blockNumber})`,
                    ).to.equal(blockNumber);

                    expect(
                        followerBlockNumber,
                        `Follower replication log block number (${followerReplicationLog.block_number}) doesn't match requested block number (${blockNumber})`,
                    ).to.equal(blockNumber);

                    // Validate that the replication logs are not null
                    expect(
                        leaderReplicationLog.replication_log,
                        `Leader replication log content for block ${blockNumber} is null or empty`,
                    ).to.not.be.null.and.to.not.be.empty;

                    expect(
                        followerReplicationLog.replication_log,
                        `Follower replication log content for block ${blockNumber} is null or empty`,
                    ).to.not.be.null.and.to.not.be.empty;

                    // Compare the replication logs
                    expect(
                        leaderReplicationLog.replication_log,
                        `Replication logs for block ${blockNumber} do not match`,
                    ).to.equal(followerReplicationLog.replication_log);
                }
            });
        });
    });

    describe("Pending storage tests", function () {
        it("Validate eth_getTransactionCount with 'pending' parameter updates correctly on follower nodes", async function () {
            this.timeout(60000);

            const testWallet = ethers.Wallet.createRandom().connect(ethers.provider);

            updateProviderUrl("stratus");
            await deployer.sendTransaction({
                to: testWallet.address,
                value: ethers.parseEther("1.0"),
                gasLimit: GAS_LIMIT_OVERRIDE,
            });

            const leaderInitialCount = await sendWithRetry("eth_getTransactionCount", [testWallet.address, "pending"]);

            updateProviderUrl("stratus-follower");
            const followerInitialCount = await sendWithRetry("eth_getTransactionCount", [
                testWallet.address,
                "pending",
            ]);

            updateProviderUrl("stratus");
            const tx = await testWallet.sendTransaction({
                to: ethers.Wallet.createRandom().address,
                value: 1,
                gasLimit: GAS_LIMIT_OVERRIDE,
                gasPrice: 0,
                type: 0,
            });

            await tx.wait();
            await waitForFollowerToSyncWithLeader();

            updateProviderUrl("stratus");
            const leaderCountAfterTx = await sendWithRetry("eth_getTransactionCount", [testWallet.address, "pending"]);

            updateProviderUrl("stratus-follower");
            const followerCountAfterTx = await sendWithRetry("eth_getTransactionCount", [
                testWallet.address,
                "pending",
            ]);

            // Verify leader incremented the nonce
            expect(parseInt(leaderCountAfterTx, 16)).to.be.greaterThan(parseInt(leaderInitialCount, 16));

            // Follower should also increment the nonce
            expect(parseInt(followerCountAfterTx, 16)).to.be.greaterThan(parseInt(followerInitialCount, 16));

            // Both nodes should return the same nonce
            expect(followerCountAfterTx).to.equal(leaderCountAfterTx);
        });

        it("Validate account balances with 'pending' parameter update correctly on follower nodes", async function () {
            this.timeout(60000);

            const testWallet = ethers.Wallet.createRandom().connect(ethers.provider);

            updateProviderUrl("stratus");
            await deployer.sendTransaction({
                to: testWallet.address,
                value: ethers.parseEther("1.0"),
                gasLimit: GAS_LIMIT_OVERRIDE,
            });

            await waitForFollowerToSyncWithLeader();

            updateProviderUrl("stratus");
            const leaderInitialBalance = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            updateProviderUrl("stratus-follower");
            const followerInitialBalance = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            updateProviderUrl("stratus");
            const tx = await testWallet.sendTransaction({
                to: ethers.Wallet.createRandom().address,
                value: ethers.parseEther("0.1"),
                gasLimit: GAS_LIMIT_OVERRIDE,
                gasPrice: 0,
                type: 0,
            });

            await tx.wait();
            await waitForFollowerToSyncWithLeader();

            updateProviderUrl("stratus");
            const leaderBalanceAfterTx = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            updateProviderUrl("stratus-follower");
            const followerBalanceAfterTx = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            // Verify balances decreased on both nodes
            expect(BigInt(leaderBalanceAfterTx)).to.be.lessThan(BigInt(leaderInitialBalance));
            expect(BigInt(followerBalanceAfterTx)).to.be.lessThan(BigInt(followerInitialBalance));

            // Both nodes should return the same balance
            expect(followerBalanceAfterTx).to.equal(leaderBalanceAfterTx);
        });

        it("Validate contract storage slots update correctly on follower nodes", async function () {
            this.timeout(60000);

            updateProviderUrl("stratus");

            const testWallet = ethers.Wallet.createRandom().connect(ethers.provider);

            const leaderInitialBalance = await brlcToken.balanceOf(testWallet.address);

            updateProviderUrl("stratus-follower");
            const followerInitialBalance = await brlcToken.balanceOf(testWallet.address);

            expect(followerInitialBalance.toString()).to.equal(leaderInitialBalance.toString());

            updateProviderUrl("stratus");
            const mintAmount = 100;
            const tx = await brlcToken.mint(testWallet.address, mintAmount, { gasLimit: GAS_LIMIT_OVERRIDE });
            await tx.wait();

            await waitForFollowerToSyncWithLeader();

            // Get token balances after minting
            updateProviderUrl("stratus");
            const leaderBalanceAfterTx = await brlcToken.balanceOf(testWallet.address);

            updateProviderUrl("stratus-follower");
            const followerBalanceAfterTx = await brlcToken.balanceOf(testWallet.address);

            // Verify balances increased on both nodes
            expect(leaderBalanceAfterTx.toString()).to.equal(
                (BigInt(leaderInitialBalance.toString()) + BigInt(mintAmount)).toString(),
            );
            expect(followerBalanceAfterTx.toString()).to.equal(
                (BigInt(followerInitialBalance.toString()) + BigInt(mintAmount)).toString(),
            );

            // Both nodes should return the same balance
            expect(followerBalanceAfterTx.toString()).to.equal(leaderBalanceAfterTx.toString());
        });

        it("Validate contract bytecode synchronization between leader and follower", async function () {
            this.timeout(60000);

            updateProviderUrl("stratus");

            const contractAddress = await brlcToken.getAddress();

            await waitForFollowerToSyncWithLeader();

            // Get bytecode from both nodes
            updateProviderUrl("stratus");
            const leaderBytecode = await sendWithRetry("eth_getCode", [contractAddress, "pending"]);

            updateProviderUrl("stratus-follower");
            const followerBytecode = await sendWithRetry("eth_getCode", [contractAddress, "pending"]);

            // Verify bytecode is the same on both nodes
            expect(followerBytecode).to.equal(leaderBytecode);
            expect(followerBytecode).to.not.equal("0x");
        });

        it("Validate multiple transactions correctly update pending state on follower nodes", async function () {
            this.timeout(120000);

            const testWallet = ethers.Wallet.createRandom().connect(ethers.provider);

            updateProviderUrl("stratus");
            await deployer.sendTransaction({
                to: testWallet.address,
                value: ethers.parseEther("2.0"),
                gasLimit: GAS_LIMIT_OVERRIDE,
            });

            await waitForFollowerToSyncWithLeader();

            const recipients = Array(5)
                .fill(0)
                .map(() => ethers.Wallet.createRandom().address);
            const txPromises = [];

            for (const recipient of recipients) {
                const tx = await testWallet.sendTransaction({
                    to: recipient,
                    value: ethers.parseEther("0.1"),
                    gasLimit: GAS_LIMIT_OVERRIDE,
                    gasPrice: 0,
                    type: 0,
                });
                txPromises.push(tx.wait());
            }

            await Promise.all(txPromises);
            await waitForFollowerToSyncWithLeader();

            // Check nonce and balance on both nodes
            updateProviderUrl("stratus");
            const leaderNonce = await sendWithRetry("eth_getTransactionCount", [testWallet.address, "pending"]);
            const leaderBalance = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            updateProviderUrl("stratus-follower");
            const followerNonce = await sendWithRetry("eth_getTransactionCount", [testWallet.address, "pending"]);
            const followerBalance = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            // Verify nonce and balance are the same on both nodes
            expect(followerNonce).to.equal(leaderNonce);
            expect(followerBalance).to.equal(leaderBalance);

            // Verify nonce increased by the number of transactions
            expect(parseInt(leaderNonce, 16)).to.equal(recipients.length);
        });
    });

    describe("Leadership switch tests", function () {
        it("Validate consistency after leadership switch", async function () {
            this.timeout(120000);

            const testWallet = ethers.Wallet.createRandom().connect(ethers.provider);

            updateProviderUrl("stratus");
            await deployer.sendTransaction({
                to: testWallet.address,
                value: ethers.parseEther("1.0"),
                gasLimit: GAS_LIMIT_OVERRIDE,
            });

            const tx = await testWallet.sendTransaction({
                to: ethers.Wallet.createRandom().address,
                value: ethers.parseEther("0.1"),
                gasLimit: GAS_LIMIT_OVERRIDE,
                gasPrice: 0,
                type: 0,
            });

            await tx.wait();
            await waitForFollowerToSyncWithLeader();

            // Get pre-switch values from both nodes
            updateProviderUrl("stratus");
            const leaderPreSwitchNonce = await sendWithRetry("eth_getTransactionCount", [
                testWallet.address,
                "pending",
            ]);
            const leaderPreSwitchBalance = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            updateProviderUrl("stratus-follower");
            const followerPreSwitchNonce = await sendWithRetry("eth_getTransactionCount", [
                testWallet.address,
                "pending",
            ]);
            const followerPreSwitchBalance = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            expect(followerPreSwitchNonce).to.equal(leaderPreSwitchNonce);
            expect(followerPreSwitchBalance).to.equal(leaderPreSwitchBalance);

            // First leadership switch
            updateProviderUrl("stratus");
            await sendWithRetry("stratus_disableTransactions", []);

            await sendWithRetry("stratus_disableMiner", []);

            await new Promise((resolve) => setTimeout(resolve, 4000));
            await sendWithRetry("stratus_changeToFollower", [
                "http://0.0.0.0:3001/",
                "ws://0.0.0.0:3001/",
                "2s",
                "100ms",
                "10485760",
            ]);

            updateProviderUrl("stratus-follower");
            await sendWithRetry("stratus_disableTransactions", []);

            await sendWithRetry("stratus_changeToLeader", []);

            await new Promise((resolve) => setTimeout(resolve, 4000));
            await sendWithRetry("stratus_enableTransactions", []);

            updateProviderUrl("stratus");
            await sendWithRetry("stratus_enableTransactions", []);

            await new Promise((resolve) => setTimeout(resolve, 8000));

            // Validate post-switch values from both nodes
            updateProviderUrl("stratus");
            const newFollowerPostSwitchNonce = await sendWithRetry("eth_getTransactionCount", [
                testWallet.address,
                "pending",
            ]);
            const newFollowerPostSwitchBalance = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            updateProviderUrl("stratus-follower");
            const newLeaderPostSwitchNonce = await sendWithRetry("eth_getTransactionCount", [
                testWallet.address,
                "pending",
            ]);
            const newLeaderPostSwitchBalance = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            expect(newLeaderPostSwitchNonce).to.equal(
                newFollowerPostSwitchNonce,
                "Transaction count mismatch after leadership switch",
            );
            expect(newLeaderPostSwitchBalance).to.equal(
                newFollowerPostSwitchBalance,
                "Balance mismatch after leadership switch",
            );

            expect(parseInt(newLeaderPostSwitchNonce, 16)).to.equal(
                parseInt(leaderPreSwitchNonce, 16),
                "Transaction count changed unexpectedly after leadership switch",
            );
            expect(BigInt(newLeaderPostSwitchBalance)).to.equal(
                BigInt(leaderPreSwitchBalance),
                "Balance changed unexpectedly after leadership switch",
            );

            updateProviderUrl("stratus-follower");
            const tx2 = await testWallet.sendTransaction({
                to: ethers.Wallet.createRandom().address,
                value: ethers.parseEther("0.1"),
                gasLimit: GAS_LIMIT_OVERRIDE,
                gasPrice: 0,
                type: 0,
            });

            await tx2.wait();
            await waitForFollowerToSyncWithLeader();

            updateProviderUrl("stratus-follower");
            const leaderAfterTx2Nonce = await sendWithRetry("eth_getTransactionCount", [testWallet.address, "pending"]);
            const leaderAfterTx2Balance = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            updateProviderUrl("stratus");
            const followerAfterTx2Nonce = await sendWithRetry("eth_getTransactionCount", [
                testWallet.address,
                "pending",
            ]);
            const followerAfterTx2Balance = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            expect(leaderAfterTx2Nonce).to.equal(
                followerAfterTx2Nonce,
                "Transaction count mismatch after second transaction",
            );
            expect(leaderAfterTx2Balance).to.equal(
                followerAfterTx2Balance,
                "Balance mismatch after second transaction",
            );

            expect(parseInt(leaderAfterTx2Nonce, 16)).to.equal(
                parseInt(newLeaderPostSwitchNonce, 16) + 1,
                "Transaction count didn't increase after second transaction",
            );
            expect(BigInt(leaderAfterTx2Balance)).to.be.lessThan(
                BigInt(newLeaderPostSwitchBalance),
                "Balance didn't decrease after second transaction",
            );

            // Second leadership switch
            updateProviderUrl("stratus-follower");
            await sendWithRetry("stratus_disableTransactions", []);

            await sendWithRetry("stratus_disableMiner", []);

            await new Promise((resolve) => setTimeout(resolve, 4000));
            await sendWithRetry("stratus_changeToFollower", [
                "http://0.0.0.0:3000/",
                "ws://0.0.0.0:3000/",
                "2s",
                "100ms",
                "10485760",
            ]);

            updateProviderUrl("stratus");
            await sendWithRetry("stratus_disableTransactions", []);

            await sendWithRetry("stratus_changeToLeader", []);

            await new Promise((resolve) => setTimeout(resolve, 4000));
            await sendWithRetry("stratus_enableTransactions", []);

            updateProviderUrl("stratus-follower");
            await sendWithRetry("stratus_enableTransactions", []);

            await new Promise((resolve) => setTimeout(resolve, 8000));

            // Validate post-switch values from both nodes
            updateProviderUrl("stratus");
            const leaderAfterSecondSwitchNonce = await sendWithRetry("eth_getTransactionCount", [
                testWallet.address,
                "pending",
            ]);
            const leaderAfterSecondSwitchBalance = await sendWithRetry("eth_getBalance", [
                testWallet.address,
                "pending",
            ]);

            updateProviderUrl("stratus-follower");
            const followerAfterSecondSwitchNonce = await sendWithRetry("eth_getTransactionCount", [
                testWallet.address,
                "pending",
            ]);
            const followerAfterSecondSwitchBalance = await sendWithRetry("eth_getBalance", [
                testWallet.address,
                "pending",
            ]);

            expect(leaderAfterSecondSwitchNonce).to.equal(
                followerAfterSecondSwitchNonce,
                "Transaction count mismatch after second leadership switch",
            );
            expect(leaderAfterSecondSwitchBalance).to.equal(
                followerAfterSecondSwitchBalance,
                "Balance mismatch after second leadership switch",
            );

            expect(leaderAfterSecondSwitchNonce).to.equal(
                leaderAfterTx2Nonce,
                "Transaction count changed unexpectedly during second leadership switch",
            );
            expect(leaderAfterSecondSwitchBalance).to.equal(
                leaderAfterTx2Balance,
                "Balance changed unexpectedly during second leadership switch",
            );

            updateProviderUrl("stratus");
            const tx3 = await testWallet.sendTransaction({
                to: ethers.Wallet.createRandom().address,
                value: ethers.parseEther("0.1"),
                gasLimit: GAS_LIMIT_OVERRIDE,
                gasPrice: 0,
                type: 0,
            });

            await tx3.wait();
            await waitForFollowerToSyncWithLeader();

            updateProviderUrl("stratus");
            const leaderAfterTx3Nonce = await sendWithRetry("eth_getTransactionCount", [testWallet.address, "pending"]);
            const leaderAfterTx3Balance = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            updateProviderUrl("stratus-follower");
            const followerAfterTx3Nonce = await sendWithRetry("eth_getTransactionCount", [
                testWallet.address,
                "pending",
            ]);
            const followerAfterTx3Balance = await sendWithRetry("eth_getBalance", [testWallet.address, "pending"]);

            expect(leaderAfterTx3Nonce).to.equal(
                followerAfterTx3Nonce,
                "Transaction count mismatch after third transaction",
            );
            expect(leaderAfterTx3Balance).to.equal(followerAfterTx3Balance, "Balance mismatch after third transaction");

            expect(parseInt(leaderAfterTx3Nonce, 16)).to.equal(
                parseInt(leaderAfterSecondSwitchNonce, 16) + 1,
                "Transaction count didn't increase after third transaction",
            );
            expect(BigInt(leaderAfterTx3Balance)).to.be.lessThan(
                BigInt(leaderAfterSecondSwitchBalance),
                "Balance didn't decrease after third transaction",
            );
        });
    });
});
