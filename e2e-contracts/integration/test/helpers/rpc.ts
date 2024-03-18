import { ContractFactory, ContractTransactionReceipt, ContractTransactionResponse, JsonRpcProvider } from "ethers"
import { upgrades, ethers, config, network } from "hardhat";
import { BRLCToken, BalanceTracker, CardPaymentProcessor, CashbackDistributor, IERC20Hookable, PixCashier, YieldStreamer } from "../../typechain-types";
import { SignerWithAddress } from "@nomicfoundation/hardhat-ethers/signers";
import { HttpNetworkConfig } from "hardhat/types";
import { readTokenAddressFromSource, recompile, replaceTokenAddress } from "./recompile";

/* Constants */
export const FAKE_32_BYTES = "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
export const FAKE_16_BYTES = "0xabcdef1234567890abcdef1234567890"
export const ZERO_ADDRESS = "0x0000000000000000000000000000000000000000"

/* Contracts instances */
export let brlcToken: BRLCToken;
export let pixCashier: PixCashier;
export let cashbackDistributor: CashbackDistributor;
export let cardPaymentProcessor: CardPaymentProcessor;
export let balanceTracker: BalanceTracker;
export let yieldStreamer: YieldStreamer;

/* Signers and Wallets */
export let deployer: SignerWithAddress;

/* Providers */
let providerUrl = (config.networks[network.name] as HttpNetworkConfig).url || "http://localhost:8545";
let ETHERJS = new JsonRpcProvider(providerUrl);

export async function setDeployer() {
    [deployer] = await ethers.getSigners();
}

export async function waitReceipt(txResponsePromise: Promise<ContractTransactionResponse>): Promise<ContractTransactionReceipt> {
    const txReceipt = await txResponsePromise;
    return txReceipt.wait() as Promise<ContractTransactionReceipt>;
}

export async function deployBRLC() {
    let brlcFactory: ContractFactory = await ethers.getContractFactory("BRLCToken");
    let deployedProxy = await upgrades.deployProxy(brlcFactory.connect(deployer), ["BRL Coin", "BRLC"]);
    await deployedProxy.waitForDeployment();
    brlcToken = deployedProxy.connect(deployer) as BRLCToken;
}

export async function configureBRLC() {
    waitReceipt(brlcToken.updateMainMinter(await deployer.getAddress()));
    waitReceipt(brlcToken.configureMinter(await deployer.getAddress(), 1000000000));
}

export async function deployPixCashier() {
    let pixFactory: ContractFactory = await ethers.getContractFactory("PixCashier");
    let deployedProxy = await upgrades.deployProxy(pixFactory.connect(deployer), [await brlcToken.getAddress()]);
    await deployedProxy.waitForDeployment();
    pixCashier = deployedProxy.connect(deployer) as PixCashier;
}

export async function configurePixCashier() {
    brlcToken.connect(deployer).configureMinter(await pixCashier.getAddress(), 1000000000);
    waitReceipt(pixCashier.grantRole(await pixCashier.CASHIER_ROLE(), await deployer.getAddress()));
}

export async function deployCashbackDistributor() {
    let cashbackFactory: ContractFactory = await ethers.getContractFactory("CashbackDistributor");
    let deployedProxy = await upgrades.deployProxy(cashbackFactory.connect(deployer));
    await deployedProxy.waitForDeployment();
    cashbackDistributor = deployedProxy.connect(deployer) as CashbackDistributor;
}

export async function configureCashbackDistributor() {
    waitReceipt(cashbackDistributor.grantRole(await cashbackDistributor.DISTRIBUTOR_ROLE(), await deployer.getAddress()));
    waitReceipt(cashbackDistributor.enable());
}

export async function deployCardPaymentProcessor() {
    let cardPaymentProcessorFactory: ContractFactory = await ethers.getContractFactory("CardPaymentProcessor");
    let deployedProxy = await upgrades.deployProxy(cardPaymentProcessorFactory.connect(deployer), [await brlcToken.getAddress()]);
    await deployedProxy.waitForDeployment();
    cardPaymentProcessor = deployedProxy.connect(deployer) as CardPaymentProcessor;
}

export async function configureCardPaymentProcessor() {
    const rateFactor = 10;
    waitReceipt(cardPaymentProcessor.grantRole(await cardPaymentProcessor.EXECUTOR_ROLE(), await deployer.getAddress()));
    waitReceipt(cardPaymentProcessor.setCashbackDistributor(await cashbackDistributor.getAddress()));
    waitReceipt(cardPaymentProcessor.setRevocationLimit(255));
    waitReceipt(cardPaymentProcessor.setCashbackRate(1.5 * rateFactor));
    waitReceipt(cardPaymentProcessor.setCashOutAccount(await deployer.getAddress()));
    waitReceipt(brlcToken.approve(await cardPaymentProcessor.getAddress(), 0xfffffffffffff));
    waitReceipt(cashbackDistributor.grantRole(await cashbackDistributor.DISTRIBUTOR_ROLE(), await cardPaymentProcessor.getAddress()));
    waitReceipt(cardPaymentProcessor.setCashbackDistributor(await cashbackDistributor.getAddress()));
    waitReceipt(cardPaymentProcessor.enableCashback());
    waitReceipt(cardPaymentProcessor.setCashOutAccount(ZERO_ADDRESS));
}

export async function deployBalanceTracker() {
    const tokenAddressInSource = readTokenAddressFromSource();
    if (tokenAddressInSource !== await brlcToken.getAddress()) {
        replaceTokenAddress(tokenAddressInSource, await brlcToken.getAddress());
        recompile();
    }

    let balanceTrackerFactory: ContractFactory = await ethers.getContractFactory("BalanceTracker");
    let deployedProxy = await upgrades.deployProxy(balanceTrackerFactory.connect(deployer));
    await deployedProxy.waitForDeployment();
    balanceTracker = deployedProxy.connect(deployer) as BalanceTracker;
}

export async function configureBalanceTracker() {
    const REVERT_POLICY = BigInt(1);
    const toHookStruct = (hook: IERC20Hookable.HookStructOutput) => ({ account: hook.account, policy: hook.policy });
    const hooksOutput = await brlcToken.getAfterTokenTransferHooks();
    const hooks: IERC20Hookable.HookStruct[] = hooksOutput.map(toHookStruct);
    const newHook = { account: await balanceTracker.getAddress(), policy: REVERT_POLICY };
    hooks.push(newHook);
    waitReceipt(brlcToken.setAfterTokenTransferHooks(hooks));
}

export async function deployYieldStreamer() {
    let yieldStreamerFactory: ContractFactory = await ethers.getContractFactory("YieldStreamer");
    let deployedProxy = await upgrades.deployProxy(yieldStreamerFactory.connect(deployer));
    await deployedProxy.waitForDeployment();
    yieldStreamer = deployedProxy.connect(deployer) as YieldStreamer;
}

export async function configureYieldStreamer() {
    waitReceipt(yieldStreamer.setBalanceTracker(await balanceTracker.getAddress()));
}