import { expect } from "chai";
import { JsonRpcProvider, Signer, Wallet } from "ethers";
import { config } from "hardhat";
import { HttpNetworkConfig } from "hardhat/types";

import { Account } from "./account";
import { CURRENT_NETWORK } from "./network";

// Sends a RPC request to the blockchain.
export async function send(method: string, params: any[] = []): Promise<any> {
    // handle special params
    for (const i in params) {
        const param = params[i];
        if (param instanceof Account) {
            params[i] = param.address;
        }
    }

    // send request
    return PROVIDER.send(method, params);
}

// Sends a RPC request to the blockchain and applies the expect function to the result.
export async function sendExpect(method: string, params: any[] = []): Promise<Chai.Assertion> {
    return expect(await send(method, params));
}

// Get the default signer to send transactions.
export function signer(privateKey: string): Signer {
    return new Wallet(privateKey, PROVIDER);
}

// Configure RPC provider according to the network.
export const PROVIDER = new JsonRpcProvider((config.networks[CURRENT_NETWORK as string] as HttpNetworkConfig).url);

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
