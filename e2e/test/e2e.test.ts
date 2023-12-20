import { expect } from "chai";
import { keccak256 } from "ethers";
import { match } from "ts-pattern";
import { Block, Transaction, TransactionReceipt } from "web3-types";

import { ALICE, BOB } from "./helpers/account";
import { CURRENT_NETWORK, Network } from "./helpers/network";
import * as rpc from "./helpers/rpc";

const CHAIN_ID_DEC = 2008;
const CHAIN_ID = "0x" + CHAIN_ID_DEC.toString(16);
const ZERO = "0x0";
const ONE = "0x1";
const HASH_EMPTY_UNCLES = "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347";
const HASH_EMPTY_TRANSACTIONS = "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421";

describe("JSON-RPC", async () => {
    describe("Metadata", async () => {
        it("eth_chainId", async () => {
            (await rpc.sendExpect("eth_chainId")).eq(CHAIN_ID);
        });
        it("net_version", async () => {
            (await rpc.sendExpect("net_version")).eq(CHAIN_ID_DEC + "");
        });
        it("web3_clientVersion", async () => {
            let client = await rpc.sendExpect("web3_clientVersion");
            match(CURRENT_NETWORK).with(Network.Ledger, () => client.deep.eq("ledger"));
        });
    });

    describe("Gas", async () => {
        it("eth_gasPrice", async () => {
            let gasPrice = await rpc.sendExpect("eth_gasPrice");
            match(CURRENT_NETWORK)
                .with(Network.Ledger, () => gasPrice.eq(ZERO))
                .otherwise(() => gasPrice.not.eq(ZERO));
        });
    });

    describe("Account", async () => {
        it("eth_getTransactionCount", async () => {
            (await rpc.sendExpect("eth_getTransactionCount", [ALICE])).eq(ZERO);
            (await rpc.sendExpect("eth_getTransactionCount", [ALICE, "latest"])).eq(ZERO);
        });
    });

    describe("Block", async () => {
        it("eth_blockNumber", async function () {
            (await rpc.sendExpect("eth_blockNumber")).eq(ZERO);
        });
        it("eth_getBlockByNumber", async function () {
            let block: Block = await rpc.send("eth_getBlockByNumber", [ZERO, true]);
            expect(block.transactions.length).eq(0);
        });
    });
});

describe("Transaction", async () => {
    describe("Empty transfer", async () => {
        let ctx: {
            txHash: string | null;
            tx: Transaction | null;
            block: Block | null;
        } = {
            txHash: null,
            tx: null,
            block: null,
        };
        it("Send transaction", async () => {
            let txSigned = await ALICE.signer().signTransaction({
                chainId: CHAIN_ID_DEC,
                to: BOB,
                value: 0,
                gasPrice: 0,
                gasLimit: 500_000,
            });
            ctx.txHash = await rpc.send("eth_sendRawTransaction", [txSigned]);
            expect(ctx.txHash).eq(keccak256(txSigned));
        });
        it("Transaction is created", async () => {
            let tx: Transaction = await rpc.send("eth_getTransactionByHash", [ctx.txHash]);
            expect(tx.from).eq(ALICE.address);
            expect(tx.to).eq(BOB.address);
            expect(tx.nonce).eq(ZERO);
            expect(tx.chainId).eq(CHAIN_ID);
            ctx.tx = tx;
        });
        it("Block is created", async () => {
            expect(await rpc.send("eth_blockNumber")).eq(ONE);

            let block: Block = await rpc.send("eth_getBlockByNumber", [ONE, true]);
            expect(block.number).eq(ONE);

            expect(block.transactionsRoot).not.eq(HASH_EMPTY_TRANSACTIONS);
            expect(block.uncles).lengthOf(0);
            expect(block.sha3Uncles).eq(HASH_EMPTY_UNCLES);

            expect(block.transactions.length).eq(1);
            expect(block.transactions[0] as Transaction).deep.eq(ctx.tx);
            ctx.block = block;
        });
        it("Receipt is created", async () => {
            let receipt: TransactionReceipt = await rpc.send("eth_getTransactionReceipt", [ctx.txHash]);
            expect(receipt.blockNumber).eq(ctx.block!.number);
            expect(receipt.blockHash).eq(ctx.block!.hash);
            expect(receipt.transactionHash).eq(ctx.txHash);
            expect(receipt.transactionIndex).eq(ZERO);
            expect(receipt.status).eq(ONE);
        });
        it("Sender nonce increased", async () => {
            expect(await rpc.send("eth_getTransactionCount", [ALICE, "latest"])).eq(ONE);
        });
        it("Receiver nonce not increased", async () => {
            expect(await rpc.send("eth_getTransactionCount", [BOB, "latest"])).eq(ZERO);
        });
    });
});
