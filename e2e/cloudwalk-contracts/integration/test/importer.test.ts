import { expect } from "chai";
import { ethers } from "hardhat";

import {
    GAS_LIMIT_OVERRIDE,
    brlcToken,
    configureBRLC,
    deployBRLC,
    deployer,
    send,
    sendWithRetry,
    setDeployer,
    updateProviderUrl,
} from "./helpers/rpc";

describe("Run With Importer integration test", function () {
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

    describe("Long duration transaction tests", function () {
        const parameters = [
            { name: "Few wallets, sufficient balance", wallets: 3, duration: 20, tps: 300, baseBalance: 2000 },
            { name: "Few wallets, insufficient balance", wallets: 2, duration: 20, tps: 300, baseBalance: 5 },
            { name: "Many wallets, sufficient balance", wallets: 20, duration: 20, tps: 300, baseBalance: 2000 },
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
                    wallets.map((wallet) => send("eth_getTransactionCount", [wallet.address, "latest"])),
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

            it(`${params.name}: Validate transaction mined delay between Stratus and Run With Importer`, async function () {
                // Get Stratus timestamps
                updateProviderUrl("stratus");
                const stratusTimestamps = await Promise.all(
                    txHashList.map(async (txHash) => {
                        const receipt = await sendWithRetry("eth_getTransactionReceipt", [txHash]);
                        const block = await sendWithRetry("eth_getBlockByNumber", [receipt.blockNumber, false]);
                        return parseInt(block.timestamp, 16);
                    }),
                );

                // Get Run With Importer timestamps
                updateProviderUrl("run-with-importer");
                const runWithImporterTimestamps = await Promise.all(
                    txHashList.map(async (txHash) => {
                        const receipt = await sendWithRetry("eth_getTransactionReceipt", [txHash], 20);
                        const block = await sendWithRetry("eth_getBlockByNumber", [receipt.blockNumber, false]);
                        return parseInt(block.timestamp, 16);
                    }),
                );

                // Total time it took for Stratus to process all the blocks containing transactions
                const stratusProcessingTime = stratusTimestamps[stratusTimestamps.length - 1] - stratusTimestamps[0];

                // Total time it took for Run With Importer to process all the blocks containing transactions
                const runWithImporterProcessingTime =
                    runWithImporterTimestamps[runWithImporterTimestamps.length - 1] - runWithImporterTimestamps[0];

                console.log(
                    
                    `          ✔ Number of transactions sent: ${txHashList.length}`,
                );
                console.log(
                    
                    `          ✔ Stratus processing time: ${stratusProcessingTime}s | Run With Importer processing time: ${runWithImporterProcessingTime}s`,
                );
            });

            it(`${params.name}: Validate balances between Stratus and Run With Importer`, async function () {
                for (let i = 0; i < wallets.length; i++) {
                    // Get Stratus balance
                    updateProviderUrl("stratus");
                    const stratusBalance = await brlcToken.balanceOf(wallets[i].address);

                    // Get Run With Importer balance
                    updateProviderUrl("run-with-importer");
                    const runWithImporterBalance = await brlcToken.balanceOf(wallets[i].address);

                    // Assert that the balances are equal
                    expect(stratusBalance).to.equal(
                        runWithImporterBalance,
                        `Wallet ${wallets[i].address} balances are not equal between Stratus and Run With Importer`,
                    );
                }
            });

            it(`${params.name}: Validate transactions were imported from Stratus to Run With Importer`, async function () {
                // Get Stratus transaction receipts
                updateProviderUrl("stratus");
                const stratusReceipts = await Promise.all(
                    txHashList.map(async (txHash) => {
                        const receipt = await send("eth_getTransactionReceipt", [txHash]);
                        return receipt;
                    }),
                );

                // Get Run With Importer transaction receipts
                updateProviderUrl("run-with-importer");
                const runWithImporterReceipts = await Promise.all(
                    txHashList.map(async (txHash) => {
                        const receipt = await send("eth_getTransactionReceipt", [txHash]);
                        return receipt;
                    }),
                );

                // Assert that all transactions were imported
                for (let i = 0; i < txHashList.length; i++) {
                    expect(stratusReceipts[i]).to.exist;
                    expect(runWithImporterReceipts[i]).to.exist;
                }
                updateProviderUrl("stratus");
            });
        });
    });
});
