import { SignerWithAddress } from "@nomicfoundation/hardhat-ethers/signers";
import axios from "axios";
import axiosRetry from "axios-retry";
import { ContractFactory, ContractTransactionReceipt, ContractTransactionResponse, JsonRpcProvider } from "ethers";
import { config, ethers, network, upgrades } from "hardhat";
import { HttpNetworkConfig } from "hardhat/types";

import {
    BRLCToken,
    BalanceTracker,
    CardPaymentProcessor,
    CashbackDistributor,
    Cashier,
    CashierShard,
    IERC20Hookable,
    YieldStreamer,
} from "../../typechain-types";
import { readTokenAddressFromSource, recompile, replaceTokenAddress } from "./recompile";

/* Constants */
export const FAKE_32_BYTES = "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
export const FAKE_16_BYTES = "0xabcdef1234567890abcdef1234567890";
export const ZERO_ADDRESS = "0x0000000000000000000000000000000000000000";

export const CHAIN_ID_DEC = 2008;
export const CHAIN_ID = toHex(CHAIN_ID_DEC);

/* Contracts instances */
export let brlcToken: BRLCToken;
export let cashier: Cashier;
export let cashierShard: CashierShard;
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

export let ETHERJS = new JsonRpcProvider(providerUrl, undefined);

export function updateProviderUrl(providerName: string) {
    switch (providerName) {
        case "stratus-follower":
            providerUrl = "http://localhost:3001?app=e2e";
            break;
        case "stratus":
            providerUrl = "http://localhost:3000?app=e2e";
            break;
        case "hardhat":
            providerUrl = "http://localhost:8545";
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

export async function waitReceipt(
    txResponsePromise: Promise<ContractTransactionResponse>,
): Promise<ContractTransactionReceipt> {
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
    const deployerAddress = await deployer.getAddress();
    const grantorRole = await brlcToken.GRANTOR_ROLE();
    const minterRole = await brlcToken.MINTER_ROLE();
    await waitReceipt(brlcToken.grantRole(grantorRole, deployerAddress, { gasLimit: GAS_LIMIT_OVERRIDE }));
    await waitReceipt(brlcToken.grantRole(minterRole, deployerAddress, { gasLimit: GAS_LIMIT_OVERRIDE }));
}

export async function deployCashier() {
    let cashierFactory: ContractFactory = await ethers.getContractFactory("Cashier");
    let deployedProxy = await upgrades.deployProxy(cashierFactory.connect(deployer), [await brlcToken.getAddress()]);
    await deployedProxy.waitForDeployment();
    cashier = deployedProxy.connect(deployer) as Cashier;
}

export async function configureCashier() {
    waitReceipt(brlcToken.connect(deployer).grantRole(await brlcToken.MINTER_ROLE(), await cashier.getAddress()));
    waitReceipt(cashier.grantRole(await cashier.CASHIER_ROLE(), await deployer.getAddress()));
}

export async function deployCashierShard() {
    let cashierShardFactory: ContractFactory = await ethers.getContractFactory("CashierShard");
    let deployedProxy = await upgrades.deployProxy(cashierShardFactory.connect(deployer), [await cashier.getAddress()]);
    await deployedProxy.waitForDeployment();
    cashierShard = deployedProxy.connect(deployer) as CashierShard;
}

export async function configureCashierShard() {
    // Nothing to do at the moment
    // Several shards can be configured if the need rises
}

export async function deployCashbackDistributor() {
    let cashbackFactory: ContractFactory = await ethers.getContractFactory("CashbackDistributor");
    let deployedProxy = await upgrades.deployProxy(cashbackFactory.connect(deployer));
    await deployedProxy.waitForDeployment();
    cashbackDistributor = deployedProxy.connect(deployer) as CashbackDistributor;
}

export async function configureCashbackDistributor() {
    waitReceipt(
        cashbackDistributor.grantRole(await cashbackDistributor.DISTRIBUTOR_ROLE(), await deployer.getAddress()),
    );
    waitReceipt(cashbackDistributor.enable());
}

export async function deployCardPaymentProcessor() {
    let cardPaymentProcessorFactory: ContractFactory = await ethers.getContractFactory("CardPaymentProcessor");
    let deployedProxy = await upgrades.deployProxy(cardPaymentProcessorFactory.connect(deployer), [
        await brlcToken.getAddress(),
    ]);
    await deployedProxy.waitForDeployment();
    cardPaymentProcessor = deployedProxy.connect(deployer) as CardPaymentProcessor;
}

export async function configureCardPaymentProcessor() {
    const rateFactor = 10;
    waitReceipt(
        cardPaymentProcessor.grantRole(await cardPaymentProcessor.EXECUTOR_ROLE(), await deployer.getAddress()),
    );
    // @ts-ignore
    waitReceipt(cardPaymentProcessor.setCashbackDistributor(await cashbackDistributor.getAddress()));
    // @ts-ignore
    waitReceipt(cardPaymentProcessor.setRevocationLimit(255));
    waitReceipt(cardPaymentProcessor.setCashbackRate(1.5 * rateFactor));
    waitReceipt(cardPaymentProcessor.setCashOutAccount(await deployer.getAddress()));
    waitReceipt(brlcToken.approve(await cardPaymentProcessor.getAddress(), 0xfffffffffffff));
    waitReceipt(
        cashbackDistributor.grantRole(
            await cashbackDistributor.DISTRIBUTOR_ROLE(),
            await cardPaymentProcessor.getAddress(),
        ),
    );
    // @ts-ignore
    waitReceipt(cardPaymentProcessor.setCashbackDistributor(await cashbackDistributor.getAddress()));
    waitReceipt(cardPaymentProcessor.enableCashback());
    waitReceipt(cardPaymentProcessor.setCashOutAccount(ZERO_ADDRESS));
}

export async function deployBalanceTracker() {
    const tokenAddressInSource = readTokenAddressFromSource();
    if (tokenAddressInSource !== (await brlcToken.getAddress())) {
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
                await new Promise((resolve) => setTimeout(resolve, delay));
            }
        } catch (error) {
            if (attempt < maxAttempts) {
                await new Promise((resolve) => setTimeout(resolve, delay));
            } else {
                throw error;
            }
        }
    }
    throw new Error(`Failed to get a non-null response from ${methodName} after ${maxAttempts} attempts.`);
}

// Converts a number to Blockchain hex representation (prefixed with 0x).
export function toHex(number: number | bigint): string {
    return "0x" + number.toString(16);
}

export async function waitForFollowerToSyncWithLeader() {
    const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

    await delay(3000);

    while (true) {
        updateProviderUrl("stratus");
        const leaderBlock = await sendWithRetry("eth_blockNumber", []);

        updateProviderUrl("stratus-follower");
        const followerBlock = await sendWithRetry("eth_blockNumber", []);

        if (parseInt(leaderBlock, 16) === parseInt(followerBlock, 16)) {
            return { leaderBlock, followerBlock };
        }

        await delay(1000);
    }
}

export async function waitForLeaderToBeAhead() {
    const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
    const blocksAhead = 10;

    while (true) {
        updateProviderUrl("stratus");
        const leaderBlock = await sendWithRetry("eth_blockNumber", []);

        updateProviderUrl("stratus-follower");
        const followerBlock = await sendWithRetry("eth_blockNumber", []);

        if (parseInt(leaderBlock, 16) > parseInt(followerBlock, 16) + blocksAhead) {
            return { leaderBlock, followerBlock };
        }

        await delay(1000);
    }
}

export async function getTransactionByHashUntilConfirmed(txHash: string, maxRetries: number = 3) {
    const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
    let txResponse = null;
    let retries = 0;

    while (retries < maxRetries) {
        txResponse = await sendAndGetFullResponse("eth_getTransactionByHash", [txHash]);

        if (txResponse.data.result) {
            return txResponse;
        }

        retries++;
        await delay(1000);
    }

    return txResponse;
}
