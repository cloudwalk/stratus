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
        it("Validate deployer is main minter", async function () {
            updateProviderUrl("stratus-follower");

            await deployBRLC();
            await configureBRLC();

            expect(deployer.address).to.equal(await brlcToken.mainMinter());
            expect(await brlcToken.isMinter(deployer.address)).to.be.true;

            updateProviderUrl("stratus");
        });
    });

    describe("Long duration transaction tests", function () {
        const parameters = [
            { name: "Few wallets, sufficient balance", wallets: 20, duration: 30, tps: 100, baseBalance: 2000 },
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

            it("Validate account balances are identical between leader and follower", async function () {
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

                // Get all wallet addresses to check balances
                const walletAddresses = wallets.map((wallet) => wallet.address);
                // Add deployer address to the list of addresses to check
                walletAddresses.push(deployer.address);

                // Iterate through each block
                for (let blockNumber = earliestBlockNumber; blockNumber <= latestBlockNumber; blockNumber++) {
                    const blockNumberHex = `0x${blockNumber.toString(16)}`;

                    // Get all leader balances
                    updateProviderUrl("stratus");
                    const leaderBalances: any = {};
                    const leaderBRLCBalances: any = {};

                    for (const address of walletAddresses) {
                        leaderBalances[address] = await sendWithRetry("eth_getBalance", [address, blockNumberHex]);

                        try {
                            leaderBRLCBalances[address] = (
                                await brlcToken.balanceOf(address, { blockTag: blockNumber })
                            ).toString();
                        } catch (error) {
                            leaderBRLCBalances[address] = null;
                        }
                    }

                    // Get all follower balances
                    updateProviderUrl("stratus-follower");
                    const followerBalances: any = {};
                    const followerBRLCBalances: any = {};

                    for (const address of walletAddresses) {
                        followerBalances[address] = await sendWithRetry("eth_getBalance", [address, blockNumberHex]);

                        try {
                            followerBRLCBalances[address] = (
                                await brlcToken.balanceOf(address, { blockTag: blockNumber })
                            ).toString();
                        } catch (error) {
                            followerBRLCBalances[address] = null;
                        }
                    }

                    for (const address of walletAddresses) {
                        // Compare native balances
                        expect(
                            leaderBalances[address],
                            `Native balance for address ${address} at block ${blockNumber} does not match between leader and follower`,
                        ).to.equal(followerBalances[address]);

                        // Compare BRLC balances
                        expect(
                            leaderBRLCBalances[address],
                            `BRLC balance for address ${address} at block ${blockNumber} does not match between leader and follower`,
                        ).to.equal(followerBRLCBalances[address]);
                    }
                }
            });
        });
    });
});
