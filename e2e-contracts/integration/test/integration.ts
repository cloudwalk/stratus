import { ethers, network, upgrades } from "hardhat";
import { expect } from "chai";
import { Contract, ContractFactory, getBytes } from "ethers";
import { loadFixture } from "@nomicfoundation/hardhat-network-helpers";
import { SignerWithAddress } from "@nomicfoundation/hardhat-ethers/signers";

let brlCoin: Contract;
let pixCashier: Contract;
let deployer: SignerWithAddress;

async function deployBRLC() {
  let brlcFactory: ContractFactory = await ethers.getContractFactory("BRLCToken");
  brlCoin = await upgrades.deployProxy(brlcFactory.connect(deployer), ["BRL Coin", "BRLC"]);
  await brlCoin.waitForDeployment();
  let brlCoinSigned = brlCoin.connect(deployer);
  brlCoinSigned.updateMasterMinter(await deployer.getAddress());
  brlCoinSigned.configureMinter(await deployer.getAddress(), 1000000000);
}

async function deployPixCashier() {
  let pixFactory: ContractFactory = await ethers.getContractFactory("PixCashier");
  pixCashier = await upgrades.deployProxy(pixFactory.connect(deployer), [await brlCoin.getAddress()]);
  await pixCashier.waitForDeployment();
  brlCoin.connect(deployer).configureMinter(await pixCashier.getAddress(), 1000000000);
  pixCashier.grantRole(await pixCashier.CASHIER_ROLE(), await deployer.getAddress());
}

describe("Integration Test", function () {
  before(async function () {
    [deployer] = await ethers.getSigners();
    await deployBRLC();
    await deployPixCashier();
  });

  it("BRLC is ready", async function () {
    expect(await brlCoin.name()).to.equal("BRL Coin");
  });

  it("PixCashier is ready", async function () {
    expect(await pixCashier.underlyingToken()).to.equal(await brlCoin.getAddress());
    expect(await brlCoin.isMinter(await pixCashier.getAddress()));
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

