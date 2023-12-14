import { expect } from "chai";
import { Block, TransactionReceipt } from "ethers";
import { match } from "ts-pattern";

import { NETWORK, Network } from "./helpers/network";
import * as rpc from "./helpers/rpc";

describe("JSON-RPC", async () => {
    describe("Metadata", async () => {
        it("eth_chainId", async () => {
            (await rpc.sendExpect("eth_chainId")).eq("0x7d8");
        });
        it("eth_chainId", async () => {
            (await rpc.sendExpect("eth_chainId")).eq("0x7d8");
        });
        it("net_version", async () => {
            (await rpc.sendExpect("net_version")).eq("2008");
        });
        it("web3_clientVersion", async () => {
            let client = await rpc.sendExpect("web3_clientVersion");
            match(NETWORK).with(Network.Ledger, () => client.eq("ledger"));
        });
    });
    describe("Gas", async () => {
        it("eth_gasPrice", async () => {
            let gasPrice = await rpc.sendExpect("eth_gasPrice");
            match(NETWORK)
                .with(Network.Ledger, () => gasPrice.eq("0x0"))
                .otherwise(() => gasPrice.not.eq("0x0"));
        });
    });
    describe("Block", async () => {
        it("eth_blockNumber", async function () {
            (await rpc.sendExpect("eth_blockNumber")).eq("0x0");
        });
        it("eth_getBlockByNumber", async function () {
            let block: Block = await rpc.send("eth_getBlockByNumber", ["0x0", true]);
            expect(block.transactions.length).eq(0);
        });
    });
});

const TRX1_HASH = "0x1c65f63cec3c3a856baf02675181e0414d7d14775ce678a2645c2a6cb0663d0e";
describe("Transactions", async () => {
    describe("Empty transfer", async () => {
        it("Send transaction", async () => {
            let tx = await rpc.signer().sendTransaction({
                to: "0x0000000000000000000000000000000000000000",
                value: 0,
                gasPrice: 0,
                gasLimit: 500_000,
            });
            console.log(tx.hash);
        });
        it("Mine block automatically", async () => {
            (await rpc.sendExpect("eth_blockNumber")).eq("0x1");
        });
        it("Transaction receipt is valid", async () => {
            let receipt: TransactionReceipt = await rpc.send("eth_getTransactionReceipt", [TRX1_HASH]);
            expect(receipt.blockNumber).eq("0x1");
            expect(receipt.status).eq("0x1");
        });
    });
});
