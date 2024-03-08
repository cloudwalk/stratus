import axios from "axios";
import { expect } from "chai";
import { JsonRpcProvider, keccak256 } from "ethers";
import { config, ethers } from "hardhat";
import { HttpNetworkConfig } from "hardhat/types";
import { Numbers } from "web3-types";
import { WebSocket, WebSocketServer } from "ws";

import { TestContractBalances, TestContractCounter } from "../../typechain-types";
import { Account, CHARLIE } from "./account";
import { currentNetwork, isStratus } from "./network";

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------
export const CHAIN_ID_DEC = 2008;
export const CHAIN_ID = toHex(CHAIN_ID_DEC);

export const TX_PARAMS = { chainId: CHAIN_ID, gasPrice: 0, gasLimit: 1_000_000 };

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
    var payloads = null;
    var kind = "";
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
var requestId = 0;
export async function sendAndGetFullResponse(method: string, params: any[] = []): Promise<any> {
    for (const i in params) {
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

// Deploys the TestContractBalances.
export async function deployTestContractBalances(): Promise<TestContractBalances> {
    const testContractFactory = await ethers.getContractFactory("TestContractBalances");
    return await testContractFactory.connect(CHARLIE.signer()).deploy();
}

// Deploys the TestContractCounter.
export async function deployTestContractCounter(): Promise<TestContractCounter> {
    const testContractFactory = await ethers.getContractFactory("TestContractCounter");
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

/// Start a subscription and returns its id
/// Waits at the most for the specified time
/// An error or timeout will result in undefined
export async function subscribeAndGetId(subscription: string, waitTimeInMilliseconds: number): Promise<string | undefined> {
    const socket = new WebSocket(providerUrl.replace("http", "ws"));
    let subsId = undefined;
    
    socket.addEventListener("open", function () {
        socket.send(JSON.stringify({ jsonrpc: "2.0", id: 0, method: "eth_subscribe", params: [subscription] }));
    });

    socket.addEventListener('message', function (event: { data: string }) {
        //console.log('Message from server ', event.data);
        if (event.data.includes("id")) {
            subsId = JSON.parse(event.data).result;
        }
        socket.close();
    });

    // Wait for the specified time, if necessary
    if (subsId === undefined && waitTimeInMilliseconds > 0)
        await new Promise(resolve => setTimeout(resolve, waitTimeInMilliseconds));

    return subsId;
}