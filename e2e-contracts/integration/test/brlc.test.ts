import { ethers } from "hardhat";
import { expect } from "chai";
import {
    GAS_LIMIT_OVERRIDE,
    brlcToken,
    configureBRLC,
    deployBRLC,
    setDeployer, 
    waitReceipt,
    deployer
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
        let alice = ethers.Wallet.createRandom().connect(ethers.provider);
        let bob = ethers.Wallet.createRandom().connect(ethers.provider);

        it("Validate mint BRLC to wallets", async function () {      
            expect(await await ethers.provider.getBalance(alice.address)).to.be.equal(0);
            expect(await await ethers.provider.getBalance(bob.address)).to.be.equal(0);

            expect(await brlcToken.mint(alice.address, 1500, { gasLimit: GAS_LIMIT_OVERRIDE })).to.have.changeTokenBalance(brlcToken, alice, 1500);
            expect(await brlcToken.mint(bob.address, 2000, { gasLimit: GAS_LIMIT_OVERRIDE })).to.have.changeTokenBalance(brlcToken, bob, 2000);

            expect(await await ethers.provider.getBalance(alice.address)).to.be.equal(1500);
            expect(await await ethers.provider.getBalance(bob.address)).to.be.equal(2000);
        });

        it("Validate transfers between wallets", async function () {            
            const transferTx = await brlcToken.connect(alice).transfer(bob.address, 50, { gasPrice: 0, gasLimit: GAS_LIMIT_OVERRIDE, type: 0 });
            console.log("Transfer result: ", transferTx);
            const transferTx2 = await brlcToken.connect(alice).transfer(bob.address, 60, { gasPrice: 0, gasLimit: GAS_LIMIT_OVERRIDE, type: 0 });
            console.log("Transfer 2 result: ", transferTx2);
        });
    });
});