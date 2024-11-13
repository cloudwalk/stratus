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
} from "./helpers/rpc";

describe("Leader & Follower BRLC integration test", function () {
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

            it(`${params.name}: Validate block contents are identical between Stratus Leader and Follower`, async function () {
                // Get unique block numbers from transaction receipts
                updateProviderUrl("stratus");
                const blockNumbers = [
                    ...new Set(
                        await Promise.all(
                            txHashList.map(async (txHash) => {
                                const receipt = await sendWithRetry("eth_getTransactionReceipt", [txHash]);
                                return receipt.blockNumber;
                            }),
                        ),
                    ),
                ];

                // Compare full block data between leader and follower
                for (const blockNumber of blockNumbers) {
                    // Get leader block
                    const leaderBlock = await sendWithRetry("eth_getBlockByNumber", [blockNumber, true]);

                    // Get follower block
                    updateProviderUrl("stratus-follower");
                    const followerBlock = await sendWithRetry("eth_getBlockByNumber", [blockNumber, true]);

                    // Compare block fields
                    expect(leaderBlock.hash, `Block ${blockNumber} hash mismatch`).to.equal(followerBlock.hash);
                    expect(leaderBlock.parentHash, `Block ${blockNumber} parentHash mismatch`).to.equal(
                        followerBlock.parentHash,
                    );
                    expect(leaderBlock.timestamp, `Block ${blockNumber} timestamp mismatch`).to.equal(
                        followerBlock.timestamp,
                    );
                    expect(leaderBlock.transactions, `Block ${blockNumber} transactions mismatch`).to.deep.equal(
                        followerBlock.transactions,
                    );

                    updateProviderUrl("stratus");
                }
            });

            it(`${params.name}: Validate balances between Stratus Leader and Follower`, async function () {
                for (let i = 0; i < wallets.length; i++) {
                    // Get Stratus Leader balance
                    updateProviderUrl("stratus");
                    const leaderBalance = await brlcToken.balanceOf(wallets[i].address);

                    // Get Stratus Follower balance
                    updateProviderUrl("stratus-follower");
                    const followerBalance = await brlcToken.balanceOf(wallets[i].address);

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
});
