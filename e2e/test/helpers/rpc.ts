import axios from "axios";
import { expect } from "chai";
import {
    BaseContract,
    Block,
    Contract,
    JsonRpcApiProviderOptions,
    JsonRpcProvider,
    keccak256,
    TransactionReceipt,
    TransactionResponse
} from "ethers";
import { config, ethers } from "hardhat";
import { HttpNetworkConfig } from "hardhat/types";
import { Numbers } from "web3-types";
import { WebSocket } from "ws";

import { TestContractBalances, TestContractCounter, TestContractDenseStorage } from "../../typechain-types";
import { Account, CHARLIE } from "./account";
import { currentMiningIntervalInMs, currentNetwork, isStratus } from "./network";

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------
export const CHAIN_ID_DEC = 2008;
export const CHAIN_ID = toHex(CHAIN_ID_DEC);

export const TX_PARAMS = { chainId: CHAIN_ID, gasPrice: 0, gasLimit: 1_000_000 };

export const HEX_PATTERN = /^0x[\da-fA-F]+$/;
export const DEFAULT_TX_TIMEOUT_IN_MS = (currentMiningIntervalInMs() ?? 1000) * 2;

// Special numbers
export const ZERO = "0x0";
export const ONE = "0x1";
export const TWO = "0x2";
export const TEST_BALANCE = "0xffffffffffffffff";
export const TEST_TRANSFER = 12345678;
export const NATIVE_TRANSFER_GAS = "0x5208"; // 21000

// Special hashes
export const HASH_ZERO = ethers.ZeroHash;
export const HASH_EMPTY_UNCLES = "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347";
export const HASH_EMPTY_TRANSACTIONS = "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421";

// -----------------------------------------------------------------------------
// Initialization
// -----------------------------------------------------------------------------

// Configure RPC provider according to the network.
let providerUrl = (config.networks[currentNetwork() as string] as HttpNetworkConfig).url;
if (!providerUrl) {
    providerUrl = "http://localhost:8545";
}

const providerOptions: JsonRpcApiProviderOptions = {
    cacheTimeout: -1, // Do not use cash request results
    batchMaxCount: 1 // Do not unite the request in batches
};

export let ETHERJS = new JsonRpcProvider(
    providerUrl,
    undefined,
    providerOptions
);

export function updateProviderUrl(newUrl: string) {
    providerUrl = newUrl;
    ETHERJS = new JsonRpcProvider(providerUrl);
}

export function getProvider() {
    return ETHERJS;
}

// Configure RPC logger if RPC_LOG env-var is configured.
function log(event: any) {
    let payloads = null;
    let kind = "";
    if (event.action == "sendRpcPayload") {
        [kind, payloads] = ["REQ  -> ", event.payload];
    }
    if (event.action == "receiveRpcResult") {
        [kind, payloads] = ["RESP <- ", event.result];
    }
    if (!Array.isArray(payloads)) {
        payloads = [payloads];
    }
    for (const payload of payloads) {
        console.log(kind, JSON.stringify(payload));
    }
}

if (process.env.RPC_LOG) {
    ETHERJS.on("debug", log).catch(error => {
        console.error("Registering listener for a debug event of the RPC failed:", error);
        process.exit(1);
    });
}

// -----------------------------------------------------------------------------
// Helper functions
// -----------------------------------------------------------------------------

// Sends an RPC request to the blockchain, returning full response.
let requestId = 0;

export async function sendAndGetFullResponse(method: string, params: any[] = []): Promise<any> {
    for (let i = 0; i < params.length; ++i) {
        const param = params[i];
        if (param instanceof Account) {
            params[i] = param.address;
        }
    }

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

// Sends an RPC request to the blockchain, returning its error field.
// Use it when you expect the RPC call to fail.
export async function sendAndGetError(method: string, params: any[] = []): Promise<any> {
    const response = await sendAndGetFullResponse(method, params);
    return response.data.error;
}

// Sends an RPC request to the blockchain and applies the expect function to the result.
export async function sendExpect(method: string, params: any[] = []): Promise<Chai.Assertion> {
    return expect(await send(method, params));
}

// Deploys the "TestContractBalances" contract.
export async function deployTestContractBalances(): Promise<TestContractBalances> {
    const testContractFactory = await ethers.getContractFactory("TestContractBalances");
    return await testContractFactory.connect(CHARLIE.signer()).deploy();
}

// Deploys the "TestContractCounter" contract.
export async function deployTestContractCounter(): Promise<TestContractCounter> {
    const testContractFactory = await ethers.getContractFactory("TestContractCounter");
    return await testContractFactory.connect(CHARLIE.signer()).deploy();
}

// Deploys the "TestContractDenseStorage" contract.
export async function deployTestContractDenseStorage(): Promise<TestContractDenseStorage> {
    const testContractFactory = await ethers.getContractFactory("TestContractDenseStorage");
    return await testContractFactory.connect(CHARLIE.signer()).deploy();
}

// Converts a number to Blockchain hex representation (prefixed with 0x).
export function toHex(number: number | bigint): string {
    return "0x" + number.toString(16);
}

export function toPaddedHex(number: number | bigint, bytes: number): string {
    return "0x" + number.toString(16).padStart(bytes * 2, "0");
}

// Converts a hex string timestamp into unix time in seconds
export function fromHexTimestamp(timestamp: Numbers): number {
    let value = timestamp.valueOf();
    if (typeof value == "string") return parseInt(value, 16);
    else throw new Error("Expected block timestamp to be a hexstring. Got: " + timestamp.valueOf());
}

// Calculate the storage position of an address key in a mapping.
// See https://docs.soliditylang.org/en/v0.8.6/internals/layout_in_storage.html#mappings-and-dynamic-arrays
export function calculateSlotPosition(address: string, slot: number): string {
    // Convert slot number and address key to 32-byte hexadecimal values
    const paddedSlotHex = slot.toString(16).padStart(64, "0");
    const paddedAddress = address.substring(2).padStart(64, "0");

    // Hash the address key with the slot to find the storage position
    const storagePosition = keccak256("0x" + paddedAddress + paddedSlotHex);

    return storagePosition;
}

// -----------------------------------------------------------------------------
// RPC methods wrappers
// -----------------------------------------------------------------------------

/// Sends a single signed transaction.
export async function sendRawTransaction(signed: string): Promise<string> {
    return await send("eth_sendRawTransaction", [signed]);
}

/// Sends multiple signed transactions in parallel.
export async function sendRawTransactions(signedTxs: string[]): Promise<string[]> {
    // shuffle
    signedTxs.sort(() => Math.random() - 0.5);

    // send
    const txHashes = [];
    for (const signedTx of signedTxs) {
        const txHash = sendRawTransaction(signedTx);
        txHashes.push(txHash);
    }
    return Promise.all(txHashes);
}

/// Resets the blockchain state to the specified block number.
export async function sendReset(): Promise<void> {
    if (isStratus) {
        await send("debug_setHead", [toHex(0)]);
    } else {
        await send("hardhat_reset");
    }
}

/// Retrieves the current nonce of an account.
export async function sendGetBalance(address: string | Account): Promise<number> {
    const result = await send("eth_getBalance", [address]);
    return parseInt(result, 16);
}

/// Retrieves the current nonce of an account.
export async function sendGetNonce(address: string | Account): Promise<number> {
    const result = await send("eth_getTransactionCount", [address]);
    return parseInt(result, 16);
}

/// Retrieves the current block number of the blockchain.
export async function sendGetBlockNumber(): Promise<number> {
    const result = await send("eth_blockNumber");
    return parseInt(result, 16);
}

/// Mines a block.
export async function sendEvmMine(): Promise<any> {
    const result = await send("evm_mine", []);
    return result;
}

/// Open a WebSocket connection and return the socket
export function openWebSocketConnection(): WebSocket {
    const socket = new WebSocket(providerUrl.replace("http", "ws"));
    return socket;
}

/// Add an "open" event listener to a WebSocket
function addOpenListener(socket: WebSocket, subscription: string, params: any) {
    socket.addEventListener("open", function () {
        socket.send(JSON.stringify({ jsonrpc: "2.0", id: 0, method: "eth_subscribe", params: [subscription, params] }));
    });
}

/// Generalized function to add a "message" event listener to a WebSocket
function addMessageListener(
    socket: WebSocket,
    messageToReturn: number,
    callback: (messageEvent: { data: string }) => Promise<void>
): Promise<any> {
    return new Promise((resolve) => {
        let messageCount = 0;
        socket.addEventListener("message", async function (messageEvent: { data: string }) {
            // console.log("Received message: " + messageEvent.data);
            messageCount++;
            if (messageCount === messageToReturn) {
                const event = JSON.parse(messageEvent.data);
                socket.close();
                resolve(event);
            } else {
                await callback(messageEvent);
                await send("evm_mine", []);
            }
        });
    });
}

/// Execute an async contract operation
async function asyncContractOperation(contract: TestContractBalances) {
    let nonce = await sendGetNonce(CHARLIE.address);
    const contractOps = contract.connect(CHARLIE.signer());
    await contractOps.add(CHARLIE.address, 10, { nonce: nonce++ });

    return new Promise(resolve => setTimeout(resolve, 1000));
}

/// Start a subscription and return its N-th event
export async function subscribeAndGetEvent(
    subscription: string,
    waitTimeInMilliseconds: number,
    messageToReturn: number = 1,
): Promise<any> {
    const socket = openWebSocketConnection();
    addOpenListener(socket, subscription, {});
    const event = await addMessageListener(socket, messageToReturn, async (_messageEvent) => {
    });

    // Wait for the specified time, if necessary
    if (waitTimeInMilliseconds > 0)
        await new Promise((resolve) => setTimeout(resolve, waitTimeInMilliseconds));

    return event;
}

/// Start a subscription with contract and return its N-th event
export async function subscribeAndGetEventWithContract(
    subscription: string,
    waitTimeInMilliseconds: number,
    messageToReturn: number = 1,
): Promise<any> {
    const contract = await deployTestContractBalances();
    const contractAddress = await contract.getAddress();
    const socket = openWebSocketConnection();
    addOpenListener(socket, subscription, { "address": contractAddress });
    const event = await addMessageListener(socket, messageToReturn, async (_messageEvent) => {
        await asyncContractOperation(contract);
    });

    // Wait for the specified time, if necessary
    if (waitTimeInMilliseconds > 0)
        await new Promise((resolve) => setTimeout(resolve, waitTimeInMilliseconds));

    return event;
}


// Prepares a signed transaction to interact with a contract
export async function prepareSignedTx(props: {
    contract: BaseContract,
    account: Account,
    methodName: string,
    methodParameters: any[]
}): Promise<string> {
    const { contract, account, methodName, methodParameters } = props;
    const nonce = await sendGetNonce(account);
    const tx = await (contract.connect(account.signer()) as Contract)[methodName].populateTransaction(
        ...methodParameters,
        {
            nonce,
            ...TX_PARAMS,
        });
    return await account.signer().signTransaction(tx);
}

// Polls for receipts of multiple transaction without checking the transaction status
export async function pollForTransactions(
    txHashes: string[],
    options: {
        timeoutInMs?: number,
        pollingIntervalInMs?: number
    } = { timeoutInMs: DEFAULT_TX_TIMEOUT_IN_MS, pollingIntervalInMs: DEFAULT_TX_TIMEOUT_IN_MS / 100 }
): Promise<TransactionReceipt[]> {
    const { timeoutInMs, pollingIntervalInMs } = normalizePollingOptions(options);
    const startTimestamp = Date.now();
    const transactionReceipts: TransactionReceipt[] = [];
    const remainingTxHashes: Set<string> = new Set(txHashes);
    while (1) {
        let requestTimeInMs = Date.now();
        const receiptPromises: Promise<TransactionReceipt | null>[] = [];
        for (const txHash of remainingTxHashes) {
            receiptPromises.push(ETHERJS.getTransactionReceipt(txHash));
        }
        const receipts = await Promise.all(receiptPromises);
        for (const receipt of receipts) {
            if (receipt) {
                remainingTxHashes.delete(receipt.hash);
                transactionReceipts.push(receipt);
            }
        }
        if (remainingTxHashes.size === 0) {
            break;
        }
        const currentTimeInMs = Date.now();
        if (currentTimeInMs - startTimestamp >= timeoutInMs) {
            throw new Error(
                `Failed to wait for transaction minting: timeout of ${timeoutInMs} ms. ` +
                `Still waiting the transactions with hashes: ${Array.from(remainingTxHashes)}`
            );
        }
        const cycleDurationInMs = currentTimeInMs - requestTimeInMs;
        if (cycleDurationInMs < pollingIntervalInMs) {
            await new Promise((resolve) => setTimeout(resolve, pollingIntervalInMs - cycleDurationInMs));
        }
    }
    const orderedTransactionReceipts: TransactionReceipt[] = [];
    txHashes.forEach(txHash => {
        const targetReceipt = transactionReceipts.find(receipt => receipt.hash == txHash);
        if (!targetReceipt) {
            throw new Error(`Failed to wait for transaction minting: a logical error with hash: ${txHash}`);
        } else {
            orderedTransactionReceipts.push(targetReceipt);
        }
    });
    return orderedTransactionReceipts;
}

// Polls for the receipt of a single transaction without checking the transaction status
export async function pollForTransaction(
    tx: TransactionResponse | string | Promise<TransactionResponse> | Promise<string> | null | undefined,
    options: {
        timeoutInMs?: number,
        pollingIntervalInMs?: number
    } = { timeoutInMs: DEFAULT_TX_TIMEOUT_IN_MS, pollingIntervalInMs: DEFAULT_TX_TIMEOUT_IN_MS / 100 }
): Promise<TransactionReceipt> {
    if (!tx) {
        throw new Error("Transaction not found");
    }
    tx = await tx;
    const txHash = (typeof tx === "string") ? tx : tx.hash;
    const [txReceipt] = await pollForTransactions([txHash], options);
    return txReceipt;
}

// Polls for the block with a given number is minted
export async function pollForBlock(
    blockNumber: number,
    options: {
        timeoutInMs?: number,
        pollingIntervalInMs?: number
    } = { timeoutInMs: DEFAULT_TX_TIMEOUT_IN_MS, pollingIntervalInMs: DEFAULT_TX_TIMEOUT_IN_MS / 100 }
): Promise<Block> {
    const { timeoutInMs, pollingIntervalInMs } = normalizePollingOptions(options);
    const startTimeInMs = Date.now();
    let requestTimeInMs = startTimeInMs;
    let block: Block | null = await ETHERJS.getBlock(blockNumber);
    while (!block) {
        if (Date.now() - startTimeInMs >= timeoutInMs) {
            throw new Error(
                `Failed to wait for minting of block ${blockNumber}: timeout of ${timeoutInMs} ms. `
            );
        }
        const cycleDurationInMs = Date.now() - requestTimeInMs;
        if (cycleDurationInMs < pollingIntervalInMs) {
            await new Promise((resolve) => setTimeout(resolve, pollingIntervalInMs - cycleDurationInMs));
        }
        requestTimeInMs = Date.now();
        block = await ETHERJS.getBlock(blockNumber);
    }
    return block;
}

// Polls for the next block is minted
export async function pollForNextBlock(
    options: {
        timeoutInMs?: number,
        pollingIntervalInMs?: number
    } = { timeoutInMs: DEFAULT_TX_TIMEOUT_IN_MS, pollingIntervalInMs: DEFAULT_TX_TIMEOUT_IN_MS / 100 }
): Promise<Block> {
    const latestBlockNumber = await sendGetBlockNumber();
    return pollForBlock(latestBlockNumber + 1, options);
}

// Normalizes the options for poll functions
function normalizePollingOptions(options: {
    timeoutInMs?: number;
    pollingIntervalInMs?: number
} = {}): { timeoutInMs: number; pollingIntervalInMs: number } {
    const timeoutInMs: number = options.timeoutInMs ? options.timeoutInMs : DEFAULT_TX_TIMEOUT_IN_MS;
    const pollingIntervalInMs: number = options.pollingIntervalInMs ? options.pollingIntervalInMs : timeoutInMs / 100;
    return {
        timeoutInMs,
        pollingIntervalInMs
    };
}
