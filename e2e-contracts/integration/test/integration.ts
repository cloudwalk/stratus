import { ethers, upgrades } from "hardhat";
import { expect } from "chai";
import { ContractFactory, toBigInt } from "ethers";
import { SignerWithAddress } from "@nomicfoundation/hardhat-ethers/signers";
import { BRLCToken, BalanceTracker, CardPaymentProcessor, CashbackDistributor, IERC20Hookable, PixCashier } from "../typechain-types";
import { readTokenAddressFromSource, recompile, replaceTokenAddress } from "./helpers/recompile";

/* Contracts instances */
let brlCoin: BRLCToken;
let pixCashier: PixCashier;
let cashbackDistributor: CashbackDistributor;
let cardPaymentProcessor: CardPaymentProcessor;
let balanceTracker: BalanceTracker;

/* Signers and Wallets */
let deployer: SignerWithAddress;

async function deployBRLC() {
  let brlcFactory: ContractFactory = await ethers.getContractFactory("BRLCToken");
  let deployedProxy = await upgrades.deployProxy(brlcFactory.connect(deployer), ["BRL Coin", "BRLC"]);
  await deployedProxy.waitForDeployment();
  brlCoin = deployedProxy.connect(deployer) as BRLCToken;
}

async function configureBRLC() {
  brlCoin.updateMainMinter(await deployer.getAddress());
  brlCoin.configureMinter(await deployer.getAddress(), 1000000000);
}

async function deployPixCashier() {
  let pixFactory: ContractFactory = await ethers.getContractFactory("PixCashier");
  let deployedProxy = await upgrades.deployProxy(pixFactory.connect(deployer), [await brlCoin.getAddress()]);
  await deployedProxy.waitForDeployment();
  pixCashier = deployedProxy.connect(deployer) as PixCashier;
}

async function configurePixCashier() {
  brlCoin.connect(deployer).configureMinter(await pixCashier.getAddress(), 1000000000);
  pixCashier.grantRole(await pixCashier.CASHIER_ROLE(), await deployer.getAddress());
}

async function deployCashbackDistributor() {
  let cashbackFactory: ContractFactory = await ethers.getContractFactory("CashbackDistributor");
  let deployedProxy = await upgrades.deployProxy(cashbackFactory.connect(deployer));
  await deployedProxy.waitForDeployment();
  cashbackDistributor = deployedProxy.connect(deployer) as CashbackDistributor;
}

async function configureCashbackDistributor() {
  cashbackDistributor.grantRole(await cashbackDistributor.DISTRIBUTOR_ROLE(), await deployer.getAddress());
  cashbackDistributor.enable();
}

async function deployCardPaymentProcessor() {
  let cardPaymentProcessorFactory: ContractFactory = await ethers.getContractFactory("CardPaymentProcessor");
  let deployedProxy = await upgrades.deployProxy(cardPaymentProcessorFactory.connect(deployer), [await brlCoin.getAddress()]);
  await deployedProxy.waitForDeployment();
  cardPaymentProcessor = deployedProxy.connect(deployer) as CardPaymentProcessor;
}

async function configureCardPaymentProcessor() {
  const rateFactor = 10;
  cardPaymentProcessor.grantRole(await cardPaymentProcessor.EXECUTOR_ROLE(), await deployer.getAddress());
  cardPaymentProcessor.setCashbackDistributor(await cashbackDistributor.getAddress());
  cardPaymentProcessor.setRevocationLimit(255);
  cardPaymentProcessor.setCashbackRate(1.5 * rateFactor);
  cardPaymentProcessor.setCashOutAccount(await deployer.getAddress());
  brlCoin.approve(await cardPaymentProcessor.getAddress(), 0xfffffffffffff);
  cashbackDistributor.grantRole(await cashbackDistributor.DISTRIBUTOR_ROLE(), await cardPaymentProcessor.getAddress());
  cardPaymentProcessor.setCashbackDistributor(await cashbackDistributor.getAddress());
  cardPaymentProcessor.enableCashback();
}

async function deployBalanceTracker() {
  const tokenAddressInSource = readTokenAddressFromSource();
  if (tokenAddressInSource !== await brlCoin.getAddress()) {
    replaceTokenAddress(tokenAddressInSource, await brlCoin.getAddress());
    recompile();
  }

  let balanceTrackerFactory: ContractFactory = await ethers.getContractFactory("BalanceTracker");
  let deployedProxy = await upgrades.deployProxy(balanceTrackerFactory.connect(deployer));
  await deployedProxy.waitForDeployment();
  balanceTracker = deployedProxy.connect(deployer) as BalanceTracker;
}

async function configureBalanceTracker() {
  //TODO: Set hooks
}

describe("Integration Test", function () {
  before(async function () {
    [deployer] = await ethers.getSigners();
  });

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
    expect(await balanceTracker.TOKEN()).to.equal(await brlCoin.getAddress());
  });

  it("Configure BalanceTracker", async function () {
    await configureBalanceTracker();
  });

  describe("Scenario 1", function () {

    let alice = ethers.Wallet.createRandom().connect(ethers.provider);
    let bob = ethers.Wallet.createRandom();

    it("Mint BRLC to Alice", async function () {
      await brlCoin.connect(deployer).mint(alice.address, 900);
    });

    it("Cash in BRLC to Alice", async function () {
      await pixCashier.connect(deployer).cashIn(alice.address, 100, "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890");
    });

    it("Alice transfers BRLC to Bob", async function () {
      await brlCoin.connect(alice).transfer(bob.address, 50, { gasPrice: 0 });
    });

    it("Alice approves PixCashier to spend BRLC", async function () {
      await brlCoin.connect(alice).approve(await pixCashier.getAddress(), 0xfffffffffffff, { gasPrice: 0 });
    });

    it("Request Pix cash out for Alice", async function () {
      await pixCashier.connect(deployer).requestCashOutFrom(alice.address, 25, "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890");
    });

    it("Confirm Alice cashout", async function () {
      await pixCashier.connect(deployer).confirmCashOut("0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890");
    });

    it("Final state is correct", async function () {
      expect(await brlCoin.balanceOf(alice.address)).to.equal(925);
      expect(await brlCoin.allowance(alice.address, await pixCashier.getAddress())).to.equal(0xfffffffffffff - 25);
      expect(await brlCoin.balanceOf(bob.address)).to.equal(50);
      expect(await brlCoin.balanceOf(await pixCashier.getAddress())).to.equal(0);
    });
  });
});

