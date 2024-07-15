import { expect } from "chai";
import { ethers } from "hardhat";

import { getDbClient } from "./helpers/db";
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

    describe("Long duration transaction tests", function () {
        const parameters = [
            { name: "Few wallets, sufficient balance", wallets: 3, duration: 5, tps: 15, baseBalance: 2000 },
            { name: "Few wallets, insufficient balance", wallets: 2, duration: 5, tps: 5, baseBalance: 5 },
            { name: "Many wallets, sufficient balance", wallets: 20, duration: 5, tps: 5, baseBalance: 2000 },
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

            it(`${params.name}: Validate transaction mined delay between consensus nodes`, async function () {
                // Get Stratus timestamps
                await Promise.all(
                    txHashList.map(async (txHash) => {
                        const receipt = await sendWithRetry("eth_getTransactionReceipt", [txHash]);
                        const block = await sendWithRetry("eth_getBlockByNumber", [receipt.blockNumber, false]);
                        //TODO check for info within getBlockByNumber
                    }),
                );
            });
        });
    });
});
