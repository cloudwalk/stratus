import { ethers, network, upgrades } from "hardhat";
import { expect } from "chai";
import { Contract, ContractFactory, getBytes } from "ethers";
import { loadFixture } from "@nomicfoundation/hardhat-network-helpers";
import { SignerWithAddress } from "@nomicfoundation/hardhat-ethers/signers";
import { BRLCToken, PixCashier } from "../typechain-types";

let brlCoin: BRLCToken;
let pixCashier: PixCashier;
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

  describe("Scenario 1", function () {

    let alice = ethers.Wallet.createRandom().connect(ethers.provider);
    let bob = ethers.Wallet.createRandom();

    it("Mint BRLC to Alice", async function () {
      await brlCoin.connect(deployer).mint(alice.address, 900);
      expect(await brlCoin.balanceOf(alice.address)).to.equal(900);
    });

    it("Pix cash in adds to Alice balance", async function () {
      await pixCashier.connect(deployer).cashIn(alice.address, 100, "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890");
      expect(await brlCoin.balanceOf(alice.address)).to.equal(1000);
    });

    it("Alice transfers BRLC to Bob", async function () {
      await brlCoin.connect(alice).transfer(bob.address, 50, { gasPrice: 0 });
      expect(await brlCoin.balanceOf(alice.address)).to.equal(950);
      expect(await brlCoin.balanceOf(bob.address)).to.equal(50);
    });

    it("Alice approves PixCashier to spend a lot of BRLC", async function () {
      await brlCoin.connect(alice).approve(await pixCashier.getAddress(), 0xfffffffffffff, { gasPrice: 0 });
      expect(await brlCoin.allowance(alice.address, await pixCashier.getAddress())).to.equal(0xfffffffffffff);
    });

    it("Pix request cashout subtracts from Alice's balance", async function () {
      await pixCashier.connect(deployer).requestCashOutFrom(alice.address, 25, "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890");
      expect(await brlCoin.balanceOf(alice.address)).to.equal(925);
    });

    it("Confirm cashout does not affect Alice's balance", async function () {
      await pixCashier.connect(deployer).confirmCashOut("0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890");
      expect(await brlCoin.balanceOf(alice.address)).to.equal(925);
    });
  });
});

