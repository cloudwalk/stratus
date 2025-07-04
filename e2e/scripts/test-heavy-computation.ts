import { ethers } from "hardhat";

async function main() {
    console.log("🚀 Starting Heavy Computation Test");
    console.log("📊 This test will deploy and execute the TestEvmInput contract with heavy loops\n");

    // Get signer
    const [signer] = await ethers.getSigners();
    console.log("👤 Using account:", signer.address);

    // Deploy TestEvmInput contract
    console.log("\n📄 Deploying TestEvmInput contract...");
    const TestEvmInput = await ethers.getContractFactory("TestEvmInput");
    const contract = await TestEvmInput.deploy();
    await contract.waitForDeployment();

    const contractAddress = await contract.getAddress();
    console.log("✅ Contract deployed at:", contractAddress);

    // Get initial state
    const initialExecutions = await contract.getExecutions();
    const initialBlock = await contract.getCurrentBlock();
    console.log("\n📊 Initial state:");
    console.log("   - Executions:", initialExecutions.toString());
    console.log("   - Current block:", initialBlock.toString());

    // Execute heavy computation
    console.log("\n⏳ Executing heavy computation (this may take several seconds)...");
    const startTime = Date.now();

    try {
        const tx = await contract.heavyComputation();
        console.log("📤 Transaction sent:", tx.hash);
        console.log("⏳ Waiting for confirmation...");

        const receipt = await tx.wait();
        const endTime = Date.now();
        const executionTime = (endTime - startTime) / 1000;

        console.log("\n✅ Transaction confirmed!");
        console.log("📊 Execution details:");
        console.log("   - Block number:", receipt.blockNumber);
        console.log("   - Gas used:", receipt.gasUsed.toString());
        console.log("   - Execution time:", executionTime.toFixed(2), "seconds");

        // Get final state
        const finalExecutions = await contract.getExecutions();
        const result = await contract.result();
        console.log("\n📊 Final state:");
        console.log("   - Executions:", finalExecutions.toString());
        console.log("   - Result:", result.toString());

        // Analyze gas cost
        const gasPrice = tx.gasPrice || ethers.parseUnits("20", "gwei");
        const gasCost = receipt.gasUsed * gasPrice;
        console.log("\n💰 Gas analysis:");
        console.log("   - Gas price:", ethers.formatUnits(gasPrice, "gwei"), "gwei");
        console.log("   - Total cost:", ethers.formatEther(gasCost), "ETH");
        console.log("   - Gas per second:", (Number(receipt.gasUsed) / executionTime).toFixed(0), "gas/s");

        // Performance summary
        console.log("\n📈 Performance Summary:");
        console.log("=".repeat(50));
        console.log("🔸 The heavy computation with 1 million iterations took", executionTime.toFixed(2), "seconds");
        console.log("🔸 This explains why transaction fees are high - the computation is extremely intensive");
        console.log("🔸 With parallel mode and multiple EVMs, this load can be distributed");
    } catch (error) {
        console.error("\n❌ Error executing heavy computation:", error);
        throw error;
    }
}

main()
    .then(() => {
        console.log("\n✅ Test completed successfully!");
        process.exit(0);
    })
    .catch((error) => {
        console.error("\n❌ Test failed:", error);
        process.exit(1);
    });
