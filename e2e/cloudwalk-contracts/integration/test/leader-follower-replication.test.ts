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

                updateProviderUrl("stratus-follower");

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

    describe("Transaction count with 'pending' parameter tests", function () {
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
    });
});
