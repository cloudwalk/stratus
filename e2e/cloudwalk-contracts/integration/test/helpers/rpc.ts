import axios from "axios";
import axiosRetry from 'axios-retry';
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

export const GAS_LIMIT_OVERRIDE = 6000000;

/* Signers and Wallets */
export let deployer: SignerWithAddress;

/* Providers */
let providerUrl = (config.networks[network.name as string] as HttpNetworkConfig).url;
if (!providerUrl) {
    providerUrl = "http://localhost:8545";
}

export let ETHERJS = new JsonRpcProvider(
    providerUrl,
    undefined
);

export function updateProviderUrl(providerName: string) {
    switch (providerName) {
        case 'stratus':
            providerUrl = 'http://localhost:3000?app=e2e';
            break;
        case 'hardhat':
            providerUrl = 'http://localhost:8545';
            break;
        default:
            throw new Error(`Unknown provider name: ${providerName}`);
    }
    ETHERJS = new JsonRpcProvider(providerUrl);
}

ETHERJS = new JsonRpcProvider(providerUrl);

export function getProvider() {
    return ETHERJS;
}

export async function setDeployer() {
    [deployer] = await ethers.getSigners();
}

export async function waitReceipt(txResponsePromise: Promise<ContractTransactionResponse>): Promise<ContractTransactionReceipt> {
    const txReceipt = await txResponsePromise;
    return txReceipt.wait() as Promise<ContractTransactionReceipt>;
}

export async function deployBRLC() {
    let brlcFactory: ContractFactory = await ethers.getContractFactory("BRLCToken");
    let deployedProxy = await upgrades.deployProxy(brlcFactory.connect(deployer), ["BRL Coin", "BRLC"], {
        txOverrides: { gasLimit: GAS_LIMIT_OVERRIDE },
    });
    await deployedProxy.waitForDeployment();
    brlcToken = deployedProxy.connect(deployer) as BRLCToken;
}

export async function configureBRLC() {
    await waitReceipt(brlcToken.updateMainMinter(await deployer.getAddress(), { gasLimit: GAS_LIMIT_OVERRIDE}));
    await waitReceipt(brlcToken.configureMinter(await deployer.getAddress(), 1000000000, { gasLimit: GAS_LIMIT_OVERRIDE}));
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

let requestId = 0;
axiosRetry(axios, { retries: 3 });

export async function sendAndGetFullResponse(method: string, params: any[] = []): Promise<any> {
    // prepare request
    const payload = {
        jsonrpc: "2.0",
        id: requestId++,
        method: method,
        params: params,
    };
    if (process.env.RPC_LOG) {
        console.log("REQ  ->", JSON.stringify(payload));
    }

    // execute request and log response
    const response = await axios.post(providerUrl, payload, { headers: { "Content-Type": "application/json" } });
    if (process.env.RPC_LOG) {
        console.log("RESP <-", JSON.stringify(response.data));
    }

    return response;
}

// Sends an RPC request to the blockchain, returning its result field.
export async function send(method: string, params: any[] = []): Promise<any> {
    const response = await sendAndGetFullResponse(method, params);
    return response.data.result;
}

// Sends an RPC request to the blockchain with retry logic, returning its result field.
export async function sendWithRetry(methodName: string, params: any[], maxAttempts = 5, delay = 2000) {
    for (let attempt = 1; attempt <= maxAttempts; attempt++) {
        try {
            const result = await send(methodName, params);
            if (result !== null) {
                return result;
            }
            if (attempt < maxAttempts) {
                await new Promise(resolve => setTimeout(resolve, delay));
            }
        } catch (error) {
            if (attempt < maxAttempts) {
                await new Promise(resolve => setTimeout(resolve, delay));
            } else {
                throw error;
            }
        }
    }
    throw new Error(`Failed to get a non-null response from ${methodName} after ${maxAttempts} attempts.`);
}
