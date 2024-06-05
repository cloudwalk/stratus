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
    describe("Transaction tests", function () {
        const parameters = [
            { wallets: 2, duration: 10, tps: 5 },
            { wallets: 100, duration: 10, tps: 30 },
        ];
        parameters.forEach((params, index) => {
            const wallets: any[] = [];
            it(`Test case ${index + 1}: Prepare and mint BRLC to wallets`, async function () {  
                this.timeout(params.wallets * 1000 + 10000);

                for (let i = 0; i < params.wallets; i++) {
                    const wallet = ethers.Wallet.createRandom().connect(ethers.provider);
                    wallets.push(wallet);
                }
    
                for (let i = 0; i < wallets.length; i++) {
                    const wallet = wallets[i];
                    expect(await ethers.provider.getBalance(wallet.address)).to.be.equal(0);
                    expect(await brlcToken.mint(wallet.address, 2000, { gasLimit: GAS_LIMIT_OVERRIDE })).to.have.changeTokenBalance(brlcToken, wallet, 2000);
                    expect(await brlcToken.balanceOf(wallet.address)).to.equal(2000);
                }
            });
    
            let txHashList: string[] = []
            it(`Test case ${index + 1}: Transfer BRLC between wallets at a configurable TPS`, async function () {
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
    
                    const tx = await brlcToken.connect(sender).transfer(receiver.address, amount, { gasPrice: 0, gasLimit: GAS_LIMIT_OVERRIDE, type: 0, nonce: nonces[senderIndex] });
                    txHashList.push(tx.hash);
    
                    nonces[senderIndex]++;
    
                    await new Promise(resolve => setTimeout(resolve, transactionInterval));
                }
                await new Promise(resolve => setTimeout(resolve, 5000));
            });
    
            it(`Test case ${index + 1}: Validate transaction mined delay between Stratus and Hardhat`, async function () {
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
    
                console.log(`WARN: Stratus processing time: ${stratusProcessingTime}s | Hardhat processing time: ${hardhatProcessingTime}s`);
            });
    
            it(`Test case ${index + 1}: Validate balances between Stratus and Hardhat`, async function () {
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
    
            it(`Test case ${index + 1}: Validate transactions were relayed from Stratus to Hardhat`, async function () {
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
    
            it(`Test case ${index + 1}: Validate no mismatched transactions were generated`, async function () {
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
});