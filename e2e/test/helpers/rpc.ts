import axios from "axios";
import { expect } from "chai";
import { JsonRpcProvider, keccak256, TransactionReceipt, TransactionResponse } from "ethers";
import { config, ethers } from "hardhat";
import { HttpNetworkConfig } from "hardhat/types";
import { Numbers } from "web3-types";
import { WebSocket } from "ws";

import { TestContractBalances, TestContractCounter } from "../../typechain-types";
import { Account, CHARLIE } from "./account";
import { BlockMode, currentBlockMode, currentMiningIntervalMs, currentNetwork, isStratus } from "./network";

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------
export const CHAIN_ID_DEC = 2008;
export const CHAIN_ID = toHex(CHAIN_ID_DEC);

export const TX_PARAMS = { chainId: CHAIN_ID, gasPrice: 0, gasLimit: 1_000_000 };

export const HEX_PATTERN = /^0x[\da-fA-F]+$/;

export const DEFAULT_TX_TIMEOUT_MS = (currentMiningIntervalMs() ?? 1000) * 2;

// Special numbers
export const ZERO = "0x0";
export const ONE = "0x1";
export const TEST_BALANCE = "0xffffffffffffffff";
export const TEST_TRANSFER = 12345678;
export const NATIVE_TRANSFER_GAS = "0x5208"; // 21000

// Special hashes
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
export const ETHERJS = new JsonRpcProvider(providerUrl);

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
    ETHERJS.on("debug", log);
}

// -----------------------------------------------------------------------------
// Helper functions
// -----------------------------------------------------------------------------

// Sends a RPC request to the blockchain, returning full response.
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

// Sends a RPC request to the blockchain, returning its result field.
export async function send(method: string, params: any[] = []): Promise<any> {
    const response = await sendAndGetFullResponse(method, params);
    return response.data.result;
}

// Sends a RPC request to the blockchain, returning its error field.
// Use it when you expect the RPC call to fail.
export async function sendAndGetError(method: string, params: any[] = []): Promise<any> {
    const response = await sendAndGetFullResponse(method, params);
    return response.data.error;
}

// Sends a RPC request to the blockchain and applies the expect function to the result.
export async function sendExpect(method: string, params: any[] = []): Promise<Chai.Assertion> {
    return expect(await send(method, params));
}

export async function mineBlockIfNeeded() {
    if (currentBlockMode() === BlockMode.External) {
        await sendEvmMine();
    }
}

// Mine a block if needed and wait for receipts of multiple transaction without checking the transaction status.
export async function waitForTransactions(
    txHashes: string[],
    timeoutMs: number = DEFAULT_TX_TIMEOUT_MS
): Promise<TransactionReceipt[]> {
    const startTimestamp = Date.now();
    const transactionReceipts: TransactionReceipt[] = [];
    const remainingTxHashes: Set<string> = new Set(txHashes);
    while (1) {
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
        if (Date.now() - startTimestamp >= timeoutMs) {
            throw new Error(
                `Failed to wait for transaction minting: timeout of ${timeoutMs} ms. ` +
                `Still waiting the transactions with hashes: ${Array.from(remainingTxHashes)}`
            );
        }
        const pause = Math.ceil(timeoutMs / 100);
        await new Promise((resolve) => setTimeout(resolve, pause));
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

export async function getTransactionResponses(txHashes: string[]): Promise<TransactionResponse[]> {
    const txResponsePromises: Promise<TransactionResponse | null>[] = [];
    for (const txHash of txHashes) {
        txResponsePromises.push(ETHERJS.getTransaction(txHash));
    }
    const txResponses: (TransactionResponse | null)[] = await Promise.all(txResponsePromises);
    const nonExistentTxHashes: string[] = [];
    for (let i = 0; i < txResponses.length; ++i) {
        if (!txResponses[i]) {
            nonExistentTxHashes.push(txHashes[i]);
        }
    }
    if (nonExistentTxHashes.length !== 0) {
        throw new Error(`Cannot find transactions with hashes: ${nonExistentTxHashes}`);
    }
    return txResponses as TransactionResponse[];
}

// Mines a block if needed and waits for minting of a single transaction then checks success minting
export async function proveTx(
    tx: TransactionResponse | Promise<TransactionResponse> | string | string[] | null | undefined,
    timeoutMs: number = DEFAULT_TX_TIMEOUT_MS
): Promise<TransactionReceipt> {
    if (!tx) {
        throw new Error("Transaction not found");
    }

    tx = await tx;
    await mineBlockIfNeeded();

    const txHash = (typeof tx === "string") ? tx : (tx as TransactionResponse).hash;

    const [txReceipt] = await waitForTransactions([txHash], timeoutMs);
    if (txReceipt.status !== 1) {
        throw new Error(`The transaction has been reverted. The hash: ${txHash}`);
    }
    return txReceipt;
}

// Mines a block if needed and waits for minting of multiple single transactions then checks success minting
export async function proveTxs(
    txs?: TransactionResponse[] | Promise<TransactionResponse[]> | string[] | Promise<string[]>,
    timeoutMs: number = DEFAULT_TX_TIMEOUT_MS
): Promise<TransactionReceipt[]> {
    if (!txs) {
        throw new Error("Transactions not found");
    }

    txs = await txs;
    await mineBlockIfNeeded();

    let txHashes: string[];
    if (typeof txs[0] === "string") {
        txHashes = txs as string[];
    } else {
        txHashes = (txs as TransactionResponse[]).map(tx => tx.hash);
    }

    const txReceipts: TransactionReceipt[] = await waitForTransactions(txHashes, timeoutMs);
    const revertedTxHashes: string[] = txReceipts
        .filter(receipt => receipt.status !== 1)
        .map(receipt => receipt.hash);
    if (revertedTxHashes.length > 0) {
        throw new Error(`Some/all transactions have been reverted. The hashes: ${revertedTxHashes}`);
    }

    return txReceipts;
}

// Deploys the TestContractBalances.
export async function deployTestContractBalances(): Promise<TestContractBalances> {
    const testContractFactory = await ethers.getContractFactory("TestContractBalances");
    const testContract = await testContractFactory.connect(CHARLIE.signer()).deploy();

    // An alternative to a single line below is to use the 'TransactionResponse.wait()' function.
    // But for some reason the alternative works much slower.
    await proveTx(testContract.deploymentTransaction());
    return testContract;
}

// Deploys the TestContractCounter.
export async function deployTestContractCounter(): Promise<TestContractCounter> {
    const testContractFactory = await ethers.getContractFactory("TestContractCounter");
    const testContract = await testContractFactory.connect(CHARLIE.signer()).deploy();
    await proveTx(testContract.deploymentTransaction());
    return testContract;
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
                send("evm_mine", []);
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
    const event = await addMessageListener(socket, messageToReturn, async (_messageEvent) => {});

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

