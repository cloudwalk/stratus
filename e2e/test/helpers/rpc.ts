import { expect } from "chai";
import { JsonRpcProvider } from "ethers";
import { config } from "hardhat";
import { HttpNetworkConfig } from "hardhat/types";

import { Account } from "./account";
import { CURRENT_NETWORK } from "./network";

// Configure RPC provider according to the network.
export const PROVIDER = new JsonRpcProvider((config.networks[CURRENT_NETWORK as string] as HttpNetworkConfig).url);

// Sends a RPC request to the blockchain.
export async function send(method: string, params: any[] = []): Promise<any> {
    for (const i in params) {
        const param = params[i];
        if (param instanceof Account) {
            params[i] = param.address;
        }
    }

    return PROVIDER.send(method, params);
}

// Sends a RPC request to the blockchain and applies the expect function to the result.
export async function sendExpect(method: string, params: any[] = []): Promise<Chai.Assertion> {
    return expect(await send(method, params));
}

// Converts a number to Blockchain hex representation (prefixed with 0x).
export function toHex(number: number): string {
    console.log("num:", number)
    return "0x" + number.toString(16);
}

// Configure RPC logger if LOG_RPC env-var is configured.
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
if (process.env.LOG_RPC) {
    PROVIDER.on("debug", log);
}
