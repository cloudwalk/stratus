import { ethers } from "hardhat";

interface TestResult {
    mode: string;
    totalTransactions: number;
    successfulTransactions: number;
    failedTransactions: number;
    totalTimeMs: number;
    averageTimeMs: number;
    transactionsPerSecond: number;
    contracts: string[];
}

async function deployMultipleContracts(count: number): Promise<string[]> {
    console.log(`\n📄 Deploying ${count} TestEvmInput contracts...`);
    const TestEvmInput = await ethers.getContractFactory("TestEvmInput");
    const contracts: string[] = [];

    for (let i = 0; i < count; i++) {
        const contract = await TestEvmInput.deploy();
        await contract.waitForDeployment();
        const address = await contract.getAddress();
        contracts.push(address);
        console.log(`   Contract ${i + 1}: ${address}`);
    }

    return contracts;
}

async function testParallelSharding(
    contractAddresses: string[],
    transactionsPerContract: number
): Promise<TestResult> {
    console.log(`\n🚀 Testing PARALLEL execution with sharding`);
    console.log(`   ${contractAddresses.length} contracts x ${transactionsPerContract} transactions each`);
    console.log(`   Total transactions: ${contractAddresses.length * transactionsPerContract}`);

    const TestEvmInput = await ethers.getContractFactory("TestEvmInput");
    const [signer] = await ethers.getSigners();

    // Get initial nonce
    const baseNonce = await signer.getNonce();
    console.log(`   Base nonce: ${baseNonce}`);

    const results: TestResult = {
        mode: "parallel-sharding",
        totalTransactions: contractAddresses.length * transactionsPerContract,
        successfulTransactions: 0,
        failedTransactions: 0,
        totalTimeMs: 0,
        averageTimeMs: 0,
        transactionsPerSecond: 0,
        contracts: contractAddresses
    };

    const startTime = Date.now();
    const promises: Promise<any>[] = [];
    let currentNonce = baseNonce;

    console.log("\n   Sending transactions to all contracts in parallel...");

    // Send transactions to different contracts with sequential nonces
    for (let txIndex = 0; txIndex < transactionsPerContract; txIndex++) {
        for (let contractIndex = 0; contractIndex < contractAddresses.length; contractIndex++) {
            const contract = TestEvmInput.attach(contractAddresses[contractIndex]);

            // Create transaction with specific nonce
            const txPromise = contract.connect(signer).heavyComputation({
                nonce: currentNonce++
            }).then(async (tx) => {
                const receipt = await tx.wait();
                results.successfulTransactions++;
                return { success: true, hash: tx.hash, contract: contractIndex };
            }).catch((error) => {
                results.failedTransactions++;
                return { success: false, error: error.message, contract: contractIndex };
            });

            promises.push(txPromise);
        }
    }

    // Wait for all transactions to complete
    const txResults = await Promise.all(promises);
    results.totalTimeMs = Date.now() - startTime;
    results.averageTimeMs = results.totalTimeMs / results.totalTransactions;
    results.transactionsPerSecond = (results.successfulTransactions * 1000) / results.totalTimeMs;

    console.log(`\n   ✅ Completed in ${results.totalTimeMs}ms`);
    console.log(`   📊 Success rate: ${results.successfulTransactions}/${results.totalTransactions}`);

    return results;
}

async function testSerialExecution(
    contractAddresses: string[],
    transactionsPerContract: number
): Promise<TestResult> {
    console.log(`\n🐌 Testing SERIAL execution`);
    console.log(`   ${contractAddresses.length} contracts x ${transactionsPerContract} transactions each`);
    console.log(`   Total transactions: ${contractAddresses.length * transactionsPerContract}`);

    const TestEvmInput = await ethers.getContractFactory("TestEvmInput");
    const [signer] = await ethers.getSigners();

    const results: TestResult = {
        mode: "serial",
        totalTransactions: contractAddresses.length * transactionsPerContract,
        successfulTransactions: 0,
        failedTransactions: 0,
        totalTimeMs: 0,
        averageTimeMs: 0,
        transactionsPerSecond: 0,
        contracts: contractAddresses
    };

    const startTime = Date.now();
    console.log("\n   Sending transactions one by one...");

    // Send transactions serially
    for (let txIndex = 0; txIndex < transactionsPerContract; txIndex++) {
        for (let contractIndex = 0; contractIndex < contractAddresses.length; contractIndex++) {
            const contract = TestEvmInput.attach(contractAddresses[contractIndex]);

            try {
                const tx = await contract.connect(signer).heavyComputation();
                await tx.wait();
                results.successfulTransactions++;

                if ((results.successfulTransactions + results.failedTransactions) % 10 === 0) {
                    console.log(`   Progress: ${results.successfulTransactions + results.failedTransactions}/${results.totalTransactions}`);
                }
            } catch (error) {
                results.failedTransactions++;
                console.error(`   Transaction failed:`, error.message);
            }
        }
    }

    results.totalTimeMs = Date.now() - startTime;
    results.averageTimeMs = results.totalTimeMs / results.totalTransactions;
    results.transactionsPerSecond = (results.successfulTransactions * 1000) / results.totalTimeMs;

    console.log(`\n   ✅ Completed in ${results.totalTimeMs}ms`);
    console.log(`   📊 Success rate: ${results.successfulTransactions}/${results.totalTransactions}`);

    return results;
}

async function printComparison(results: TestResult[]) {
    console.log("\n" + "=".repeat(80));
    console.log("📊 SHARDING PERFORMANCE COMPARISON");
    console.log("=".repeat(80));

    for (const result of results) {
        console.log(`\n🔸 ${result.mode.toUpperCase()}`);
        console.log(`   Contracts: ${result.contracts.length}`);
        console.log(`   Total transactions: ${result.totalTransactions}`);
        console.log(`   ✅ Successful: ${result.successfulTransactions}`);
        console.log(`   ❌ Failed: ${result.failedTransactions}`);
        console.log(`   ⏱️  Total time: ${result.totalTimeMs}ms`);
        console.log(`   ⚡ Average time per tx: ${result.averageTimeMs.toFixed(2)}ms`);
        console.log(`   📊 Transactions per second: ${result.transactionsPerSecond.toFixed(2)} tx/s`);
    }

    // Calculate speedup
    const serialResult = results.find(r => r.mode === "serial");
    const parallelResult = results.find(r => r.mode === "parallel-sharding");

    if (serialResult && parallelResult) {
        const speedup = parallelResult.transactionsPerSecond / serialResult.transactionsPerSecond;
        console.log("\n📈 SPEEDUP ANALYSIS");
        console.log("=".repeat(40));
        console.log(`Parallel sharding is ${speedup.toFixed(2)}x faster than serial execution`);
        console.log(`Time reduction: ${((1 - parallelResult.totalTimeMs / serialResult.totalTimeMs) * 100).toFixed(1)}%`);
    }
}

async function main() {
    const contractCount = parseInt(process.env.CONTRACT_COUNT || "5");
    const txPerContract = parseInt(process.env.TX_PER_CONTRACT || "10");

    console.log("\n🎯 PARALLEL vs SERIAL EXECUTION TEST (with Sharding)");
    console.log("=".repeat(60));
    console.log(`📊 Configuration:`);
    console.log(`   - Contracts to deploy: ${contractCount}`);
    console.log(`   - Transactions per contract: ${txPerContract}`);
    console.log(`   - Total transactions: ${contractCount * txPerContract}`);

    // Deploy contracts
    const contractAddresses = await deployMultipleContracts(contractCount);

    const results: TestResult[] = [];

    // Test serial execution
    const serialResult = await testSerialExecution(contractAddresses, txPerContract);
    results.push(serialResult);

    // Small delay between tests
    console.log("\n⏳ Waiting 2 seconds before parallel test...");
    await new Promise(resolve => setTimeout(resolve, 2000));

    // Test parallel execution with sharding
    const parallelResult = await testParallelSharding(contractAddresses, txPerContract);
    results.push(parallelResult);

    // Print comparison
    await printComparison(results);

    // Save results
    const fs = await import("fs");
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const outputFile = `sharding-test-results-${timestamp}.json`;
    await fs.promises.writeFile(outputFile, JSON.stringify({
        timestamp: new Date().toISOString(),
        configuration: {
            contractCount,
            txPerContract,
            totalTransactions: contractCount * txPerContract
        },
        results
    }, null, 2));
    console.log(`\n💾 Results saved to: ${outputFile}`);
}

main()
    .then(() => {
        console.log("\n✅ Sharding test completed successfully!");
        process.exit(0);
    })
    .catch((error) => {
        console.error("\n❌ Test failed:", error);
        process.exit(1);
    });
