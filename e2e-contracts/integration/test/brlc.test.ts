import { config, ethers, network, upgrades } from "hardhat";
import { expect } from "chai";
import { ContractFactory, JsonRpcProvider } from "ethers";
import { SignerWithAddress } from "@nomicfoundation/hardhat-ethers/signers";
import { BRLCToken, BalanceTracker, CardPaymentProcessor, CashbackDistributor, IERC20Hookable, PixCashier, YieldStreamer } from "../typechain-types";
import { readTokenAddressFromSource, recompile, replaceTokenAddress } from "./helpers/recompile";
import { HttpNetworkConfig } from "hardhat/types";
import {
    FAKE_16_BYTES, FAKE_32_BYTES, ZERO_ADDRESS,
    balanceTracker, brlcToken, cardPaymentProcessor, pixCashier,
    configureBRLC, configureBalanceTracker, configureCardPaymentProcessor, configureCashbackDistributor, configurePixCashier, configureYieldStreamer,
    deployBRLC, deployBalanceTracker, deployCardPaymentProcessor, deployCashbackDistributor, deployPixCashier, deployYieldStreamer,
    setDeployer, waitReceipt,
    deployer
} from "./helpers/rpc";
import { deploy } from "@openzeppelin/hardhat-upgrades/dist/utils";

describe("Integration Test", function () {
    before(async function () {
        await setDeployer();
    });

    describe("Deploy and configure contracts", function () {
        it("Deploy BRLC", async function () {
            await deployBRLC();
        });

        it("Configure BRLC", async function () {
            await configureBRLC();
        });

    });
    describe("Scenario 1", function () {

        let alice = ethers.Wallet.createRandom().connect(ethers.provider);

        it("Mint BRLC to Alice", async function () {
            const aliceNonce = await ethers.provider.getTransactionCount(alice.address);
            console.log("Alice nonce: ", aliceNonce);

            let deployerNonce = await ethers.provider.getTransactionCount(deployer.address);
            console.log("Deployer nonce: ", deployerNonce);

            const mintaTx = waitReceipt(brlcToken.mint(alice.address, 900, { gasLimit: 5000000, nonce: deployerNonce}));
            console.log("Mint Tx: ", mintaTx);

            const aliceBalance = await brlcToken.balanceOf(alice.address)
            console.log("Alice balance: ", aliceBalance);
        });
    });
});
