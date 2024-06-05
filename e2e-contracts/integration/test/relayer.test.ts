import { ethers } from "hardhat";
import { expect } from "chai";
import { getDbClient } from './helpers/db';
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

    describe("Long duration transaction tests", function () {
        const parameters = [
            { name: "Few wallets, sufficient balance", wallets: 3, duration: 15, tps: 5, baseBalance: 2000 },
            { name: "Few wallets, insufficient balance", wallets: 2, duration: 15, tps: 1, baseBalance: 5 },
            { name: "Many wallets, sufficient balance", wallets: 20, duration: 15, tps: 30, baseBalance: 2000 },
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
                    expect(await brlcToken.mint(wallet.address, params.baseBalance, { gasLimit: GAS_LIMIT_OVERRIDE })).to.have.changeTokenBalance(brlcToken, wallet, params.baseBalance);
                }
            });
    
            let txHashList: string[] = []
            it(`${params.name}: Transfer BRLC between wallets at a configurable TPS`, async function () {
                this.timeout(params.duration * 1000 + 10000);
    
                const transactionInterval = 1000 / params.tps;
    
                let nonces = await Promise.all(wallets.map(wallet => send("eth_getTransactionCount", [wallet.address, "latest"])));
    
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
                        const tx = await brlcToken.connect(sender).transfer(receiver.address, amount, { gasPrice: 0, gasLimit: GAS_LIMIT_OVERRIDE, type: 0, nonce: nonces[senderIndex] });
                        txHashList.push(tx.hash);
                    } catch (error) {}
    
                    nonces[senderIndex]++;
    
                    await new Promise(resolve => setTimeout(resolve, transactionInterval));
                }
                await new Promise(resolve => setTimeout(resolve, 2000));
            });
    
            it(`${params.name}: Validate transaction mined delay between Stratus and Hardhat`, async function () {
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
    
                // Total time it took for Stratus to process all the blocks containing transactions
                const stratusProcessingTime = stratusTimestamps[stratusTimestamps.length - 1] - stratusTimestamps[0];
    
                // Total time it took for Hardhat to process all the blocks containing transactions
                const hardhatProcessingTime = hardhatTimestamps[hardhatTimestamps.length - 1] - hardhatTimestamps[0];
    
                console.log(`          WARN: Stratus processing time: ${stratusProcessingTime}s | Hardhat processing time: ${hardhatProcessingTime}s`);
            });
    
            it(`${params.name}: Validate balances between Stratus and Hardhat`, async function () {
                for (let i = 0; i < wallets.length; i++) {
                    // Get Stratus balance
                    updateProviderUrl("stratus");
                    const stratusBalance = await brlcToken.balanceOf(wallets[i].address);
    
                    // Get Hardhat balance
                    updateProviderUrl("hardhat");
                    const hardhatBalance = await brlcToken.balanceOf(wallets[i].address);
    
                    // Assert that the balances are equal
                    expect(stratusBalance).to.equal(hardhatBalance, `Wallet ${wallets[i].address} balances are not equal between Stratus and Hardhat`);
                }
            });
    
            it(`${params.name}: Validate transactions were relayed from Stratus to Hardhat`, async function () {
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
    
            it(`${params.name}: Validate no mismatched transactions were generated`, async function () {
                const client = await getDbClient();
            
                // Validate connection to Postgres
                const testRes = await client.query('SELECT 1');
                expect(testRes.rowCount).to.equal(1, 'Could not connect to the database');
            
                // Validate that no mismatched transactions were found
                const res = await client.query('SELECT * FROM mismatches');
                expect(res.rows.length).to.equal(0, 'Mismatched transactions were found');
            
                await client.end();
            });
        });
    });

    describe("Edge case transaction test", function () {
        it("Back and forth transfer with minimum funds should order successfully", async function () {
            const alice = ethers.Wallet.createRandom().connect(ethers.provider);
            const bob = ethers.Wallet.createRandom().connect(ethers.provider);

            let wallets = [alice, bob];

            // Mint 10 tokens to Alice's account only
            expect(await brlcToken.mint(alice.address, 10, { gasLimit: GAS_LIMIT_OVERRIDE })).to.have.changeTokenBalance(brlcToken, alice, 10);

            let nonces = await Promise.all(wallets.map(wallet => send("eth_getTransactionCount", [wallet.address, "latest"])));

            // Perform 10 transfers back and forth between Alice and Bob
            for (let i = 0; i < 10; i++) {
                let sender = wallets[i % 2];
                let receiver = wallets[(i + 1) % 2];
                
                await brlcToken.connect(sender).transfer(receiver.address, 10, { gasPrice: 0, gasLimit: GAS_LIMIT_OVERRIDE, type: 0, nonce: nonces[i % 2] });

                nonces[i % 2]++;
            }
            await new Promise(resolve => setTimeout(resolve, 2000));
        });
    });
});