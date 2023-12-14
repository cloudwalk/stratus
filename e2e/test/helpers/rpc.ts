import { expect } from "chai";
import { JsonRpcProvider, Signer, Wallet } from "ethers";
import { config } from "hardhat";
import { HttpNetworkConfig } from "hardhat/types";

import { NETWORK } from "./network";

// Configure RPC provider according to the network.
function log(event: any) {
    var payloads = null;
    var kind = "";
    if (event.action == "sendRpcPayload") {
        [kind, payloads] = ["REQ: ", event.payload];
    }
    if (event.action == "receiveRpcResult") {
        [kind, payloads] = ["RESP: ", event.result];
    }
    if (!Array.isArray(payloads)) {
        payloads = [payloads];
    }
    for (const payload of payloads) {
        console.log(kind, JSON.stringify(payload));
    }
}
const PROVIDER = new JsonRpcProvider((config.networks[NETWORK as string] as HttpNetworkConfig).url);
if (process.env.LOG_RPC) {
    PROVIDER.on("debug", log);
}

// Sends a RPC request to the blockchain.
export async function send(method: string, params: any[] = []): Promise<any> {
    return PROVIDER.send(method, params);
}

// Sends a RPC request to the blockchain and applies the expect function to the result.
export async function sendExpect(method: string, params: any[] = []): Promise<Chai.Assertion> {
    return expect(await send(method, params));
}

// Get the default signer to send transactions.
export function signer(): Signer {
    return new Wallet("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80", PROVIDER);
}
