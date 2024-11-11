import { SignerWithAddress } from "@nomicfoundation/hardhat-ethers/signers";
import { expect } from "chai";
import { ContractFactory, JsonRpcProvider } from "ethers";
import { config, ethers, network, upgrades } from "hardhat";
import { HttpNetworkConfig } from "hardhat/types";

import { readTokenAddressFromSource, recompile, replaceTokenAddress } from "./helpers/recompile";
import {
    FAKE_16_BYTES,
    FAKE_32_BYTES,
    ZERO_ADDRESS,
    balanceTracker,
    brlcToken,
    cardPaymentProcessor,
    cashier,
    configureBRLC,
    configureBalanceTracker,
    configureCardPaymentProcessor,
    configureCashbackDistributor,
    configureCashier,
    configureCashierShard,
    configureYieldStreamer,
    deployBRLC,
    deployBalanceTracker,
    deployCardPaymentProcessor,
    deployCashbackDistributor,
    deployCashier,
    deployCashierShard,
    deployYieldStreamer,
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

        it("Deploy Cashier", async function () {
            await deployCashier();
        });

        it("Configure Cashier", async function () {
            await configureCashier();
        });

        it("Deploy CashierShard", async function () {
            await deployCashierShard();
        });

        it("Configure CashierShard", async function () {
            await configureCashierShard();
        });

        it("Deploy CashbackDistributor", async function () {
            await deployCashbackDistributor();
        });

        xit("Configure CashbackDistributor", async function () {
            await configureCashbackDistributor();
        });

        xit("Deploy CardPaymentProcessor", async function () {
            await deployCardPaymentProcessor();
        });

        xit("Configure CardPaymentProcessor", async function () {
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
            /* Using Cashier is enough for this test to pass
              and for the basic deploy/use documentation purpose of this file
              Sharding is already tested in CI */
            waitReceipt(cashier.cashIn(alice.address, 100, FAKE_32_BYTES));
        });

        it("Alice transfers BRLC to Bob", async function () {
            const x = await waitReceipt(brlcToken.connect(alice).transfer(bob.address, 50, { gasPrice: 0 }));
            expect(x.status).to.equal(1);
        });

        it("Alice approves Cashier to spend BRLC", async function () {
            const x = await waitReceipt(
                brlcToken.connect(alice).approve(await cashier.getAddress(), 0xfffffffffffff, { gasPrice: 0 }),
            );
            expect(x.status).to.equal(1);
        });

        it("Request Pix cash out for Alice", async function () {
            waitReceipt(cashier.requestCashOutFrom(alice.address, 25, FAKE_32_BYTES));
        });

        it("Confirm Alice cashout", async function () {
            waitReceipt(cashier.confirmCashOut(FAKE_32_BYTES));
        });

        xit("Alice approves CardPaymentProcessor to spend BRLC", async function () {
            const x = await waitReceipt(
                brlcToken
                    .connect(alice)
                    .approve(await cardPaymentProcessor.getAddress(), 0xfffffffffffff, { gasPrice: 0 }),
            );
            expect(x.status).to.equal(1);
        });

        xit("Make card payment for alice", async function () {
            waitReceipt(
                cardPaymentProcessor.makePaymentFor(FAKE_32_BYTES, alice.address, 15, 0, FAKE_16_BYTES, 0, 0, 0),
            );
        });

        xit("Final state is correct", async function () {
            expect(await brlcToken.balanceOf(alice.address)).to.equal(910);
            expect(await brlcToken.allowance(alice.address, await cashier.getAddress())).to.equal(0xfffffffffffff - 25);
            expect(await brlcToken.balanceOf(bob.address)).to.equal(50);
            expect(await brlcToken.balanceOf(await cashier.getAddress())).to.equal(0);
            expect(await brlcToken.balanceOf(await cardPaymentProcessor.getAddress())).to.equal(15);
        });
    });
});
