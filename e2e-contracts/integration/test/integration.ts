import { ethers, upgrades } from "hardhat";
import { expect } from "chai";
import { ContractFactory } from "ethers";
import { SignerWithAddress } from "@nomicfoundation/hardhat-ethers/signers";
import { BRLCToken, CashbackDistributor, PixCashier } from "../typechain-types";

/* Contracts instances */
let brlCoin: BRLCToken;
let pixCashier: PixCashier;
let cashbackDistributor: CashbackDistributor;

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

