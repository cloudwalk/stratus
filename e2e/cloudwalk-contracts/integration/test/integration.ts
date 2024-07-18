import { SignerWithAddress } from "@nomicfoundation/hardhat-ethers/signers";
import { expect } from "chai";
import { ContractFactory, JsonRpcProvider } from "ethers";
import { config, ethers, network, upgrades } from "hardhat";
import { HttpNetworkConfig } from "hardhat/types";

import {
    BRLCToken,
    BalanceTracker,
    CardPaymentProcessor,
    CashbackDistributor,
    IERC20Hookable,
    PixCashier,
    YieldStreamer,
} from "../typechain-types";
import { readTokenAddressFromSource, recompile, replaceTokenAddress } from "./helpers/recompile";
import {
    FAKE_16_BYTES,
    FAKE_32_BYTES,
    ZERO_ADDRESS,
    balanceTracker,
    brlcToken,
    cardPaymentProcessor,
    configureBRLC,
    configureBalanceTracker,
    configureCardPaymentProcessor,
    configureCashbackDistributor,
    configurePixCashier,
    configureYieldStreamer,
    deployBRLC,
    deployBalanceTracker,
    deployCardPaymentProcessor,
    deployCashbackDistributor,
    deployPixCashier,
    deployYieldStreamer,
    pixCashier,
    setDeployer,
    waitReceipt,
} from "./helpers/rpc";

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

        it("Deploy PixCashier", async function () {
            await deployPixCashier();
        });

        it("Configure PixCashier", async function () {
            await configurePixCashier();
        });

        it("Deploy CashbackDistributor", async function () {
            await deployCashbackDistributor();
        });

        it("Configure CashbackDistributor", async function () {
            await configureCashbackDistributor();
        });

        it("Deploy CardPaymentProcessor", async function () {
            await deployCardPaymentProcessor();
        });

        it("Configure CardPaymentProcessor", async function () {
            await configureCardPaymentProcessor();
        });

        it("Deploy BalanceTracker", async function () {
            await deployBalanceTracker();
            expect(await balanceTracker.TOKEN()).to.equal(await brlcToken.getAddress());
        });

        it("Configure BalanceTracker", async function () {
            await configureBalanceTracker();
        });

        it("Deploy YieldStreamer", async function () {
            await deployYieldStreamer();
        });

        it("Configure YieldStreamer", async function () {
            await configureYieldStreamer();
        });
    });
    describe("Scenario 1", function () {
        let alice = ethers.Wallet.createRandom().connect(ethers.provider);
        let bob = ethers.Wallet.createRandom();

        it("Mint BRLC to Alice", async function () {
            waitReceipt(brlcToken.mint(alice.address, 900));
        });

        it("Cash in BRLC to Alice", async function () {
            waitReceipt(pixCashier.cashIn(alice.address, 100, FAKE_32_BYTES));
        });

        it("Alice transfers BRLC to Bob", async function () {
            const x = await waitReceipt(brlcToken.connect(alice).transfer(bob.address, 50, { gasPrice: 0 }));
            expect(x.status).to.equal(1);
        });

        it("Alice approves PixCashier to spend BRLC", async function () {
            const x = await waitReceipt(
                brlcToken.connect(alice).approve(await pixCashier.getAddress(), 0xfffffffffffff, { gasPrice: 0 }),
            );
            expect(x.status).to.equal(1);
        });

        it("Request Pix cash out for Alice", async function () {
            waitReceipt(pixCashier.requestCashOutFrom(alice.address, 25, FAKE_32_BYTES));
        });

        it("Confirm Alice cashout", async function () {
            waitReceipt(pixCashier.confirmCashOut(FAKE_32_BYTES));
        });

        it("Alice approves CardPaymentProcessor to spend BRLC", async function () {
            const x = await waitReceipt(
                brlcToken
                    .connect(alice)
                    .approve(await cardPaymentProcessor.getAddress(), 0xfffffffffffff, { gasPrice: 0 }),
            );
            expect(x.status).to.equal(1);
        });

        it("Make card payment for alice", async function () {
            waitReceipt(
                cardPaymentProcessor.makePaymentFor(
                    alice.address,
                    15,
                    0,
                    FAKE_16_BYTES,
                    FAKE_16_BYTES,
                    ZERO_ADDRESS,
                    0,
                    0,
                ),
            );
        });

        it("Final state is correct", async function () {
            expect(await brlcToken.balanceOf(alice.address)).to.equal(910);
            expect(await brlcToken.allowance(alice.address, await pixCashier.getAddress())).to.equal(
                0xfffffffffffff - 25,
            );
            expect(await brlcToken.balanceOf(bob.address)).to.equal(50);
            expect(await brlcToken.balanceOf(await pixCashier.getAddress())).to.equal(0);
            expect(await brlcToken.balanceOf(await cardPaymentProcessor.getAddress())).to.equal(15);
        });
    });
});
