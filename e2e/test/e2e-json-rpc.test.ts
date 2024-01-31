import { expect } from "chai";
import { match } from "ts-pattern";
import { Block, Transaction, TransactionReceipt } from "web3-types";

import { ALICE } from "./helpers/account";
import { CURRENT_NETWORK, Network } from "./helpers/network";
import { CHAIN_ID, CHAIN_ID_DEC, ZERO, send, sendExpect } from "./helpers/rpc";

describe("JSON-RPC", () => {
    describe("State", () => {
        it("debug_setHead", async () => {
            (await sendExpect("debug_setHead", [ZERO])).eq(ZERO);
        });
    });

    describe("Metadata", () => {
        it("eth_chainId", async () => {
            (await sendExpect("eth_chainId")).eq(CHAIN_ID);
        });
        it("net_listening", async () => {
            (await sendExpect("net_listening")).eq("true");
        });
        it("net_version", async () => {
            (await sendExpect("net_version")).eq(CHAIN_ID_DEC + "");
        });
        it("web3_clientVersion", async () => {
            let client = await sendExpect("web3_clientVersion");
            match(CURRENT_NETWORK).with(Network.Stratus, () => client.deep.eq("stratus"));
        });
    });

    describe("Gas", () => {
        it("eth_gasPrice", async () => {
            let gasPrice = await sendExpect("eth_gasPrice");
            match(CURRENT_NETWORK)
                .with(Network.Stratus, () => gasPrice.eq(ZERO))
                .otherwise(() => gasPrice.not.eq(ZERO));
        });
    });

    describe("Account", () => {
        it("eth_getTransactionCount", async () => {
            (await sendExpect("eth_getTransactionCount", [ALICE])).eq(ZERO);
            (await sendExpect("eth_getTransactionCount", [ALICE, "latest"])).eq(ZERO);
        });
    });

    describe("Block", () => {
        it("eth_blockNumber", async function () {
            (await sendExpect("eth_blockNumber")).eq(ZERO);
        });
        it("eth_getBlockByNumber", async function () {
            let block: Block = await send("eth_getBlockByNumber", [ZERO, true]);
            expect(block.transactions.length).eq(0);
        });
        it("eth_getUncleByBlockHashAndIndex", async function () {
            let block: Block = await send("eth_getUncleByBlockHashAndIndex", [ZERO, ZERO]);
            expect(block).eq(null);
        });
    });
});
