import axios from "axios";
import { expect } from "chai";
import { JsonRpcProvider, keccak256 } from "ethers";
import { config, ethers } from "hardhat";
import { HttpNetworkConfig } from "hardhat/types";

import { TestContractBalances, TestContractCounter } from "../../typechain-types";
import { Account, CHARLIE } from "./account";
import { CURRENT_NETWORK } from "./network";

// -----------------------------------------------------------------------------
// Initialization
// -----------------------------------------------------------------------------

// Configure RPC provider according to the network.
let providerUrl = (config.networks[CURRENT_NETWORK as string] as HttpNetworkConfig).url;
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

// Sends a RPC request to the blockchain.
var requestId = 0;
export async function send(method: string, params: any[] = []): Promise<any> {
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

    return response.data.result;
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

// Calculate the storage position of an address key in a mapping.
// See https://docs.soliditylang.org/en/v0.8.6/internals/layout_in_storage.html#mappings-and-dynamic-arrays
export function calculateAddressStoragePosition(address: string, slot: number): string {
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

/// Retrieves the current nonce of an account.
export async function sendRawTransaction(signed: string): Promise<string> {
    return await send("eth_sendRawTransaction", [signed]);
}

/// Retrieves the current nonce of an account.
export async function getNonce(address: string): Promise<number> {
    const result = await send("eth_getTransactionCount", [address]);
    return parseInt(result, 16);
}

/// Retrieves the current block number of the blockchain.
export async function getBlockNumber(): Promise<number> {
    const result = await send("eth_blockNumber");
    return parseInt(result, 16);
}
