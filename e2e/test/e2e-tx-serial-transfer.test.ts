import { expect } from "chai";
import { keccak256 } from "ethers";
import { Block, Transaction, TransactionReceipt } from "web3-types";

import { ALICE, BOB } from "./helpers/account";
import {
    CHAIN_ID,
    CHAIN_ID_DEC,
    HASH_EMPTY_TRANSACTIONS,
    HASH_EMPTY_UNCLES,
    ONE,
    ZERO,
    fromHexTimestamp,
    send,
    sendRawTransaction,
    sendReset,
} from "./helpers/rpc";

describe("Transaction: serial transfer", () => {
    var _tx: Transaction;
    var _txHash: string;
    var _block: Block;
    var _txTimestampInSeconds: number;

    it("Resets blockchain", async () => {
        await sendReset();
    });
    it("Send transaction", async () => {
        let txSigned = await ALICE.signWeiTransfer(BOB.address, 0);
        _txTimestampInSeconds = Math.floor(Date.now() / 1000);
        _txHash = await sendRawTransaction(txSigned);
        expect(_txHash).eq(keccak256(txSigned));
    });
    it("Transaction is created", async () => {
        _tx = await send("eth_getTransactionByHash", [_txHash]);
        expect(_tx.from).eq(ALICE.address, "tx.from");
        expect(_tx.to).eq(BOB.address, "tx.to");
        expect(_tx.nonce).eq(ZERO, "tx.nonce");
        expect(_tx.chainId).eq(CHAIN_ID, "tx.chainId");
    });
    it("Block is created", async () => {
        expect(await send("eth_blockNumber")).eq(ONE);

        _block = await send("eth_getBlockByNumber", [ONE, true]);
        expect(_block.number).eq(ONE);

        expect(_block.transactionsRoot).not.eq(HASH_EMPTY_TRANSACTIONS);
        expect(_block.uncles).lengthOf(0);
        expect(_block.sha3Uncles).eq(HASH_EMPTY_UNCLES);

        expect(_block.transactions.length).eq(1);
        expect(_block.transactions[0] as Transaction).deep.eq(_tx);

        // Timestamp is within transaction sending time and now
        expect(fromHexTimestamp(_block.timestamp)).gte(_txTimestampInSeconds);
        expect(fromHexTimestamp(_block.timestamp)).lte(Date.now());
    });
    it("Receipt is created", async () => {
        let receipt: TransactionReceipt = await send("eth_getTransactionReceipt", [_txHash]);
        expect(receipt.blockNumber).eq(_block.number, "receipt.blockNumber");
        expect(receipt.blockHash).eq(_block.hash, "receipt.blockHash");
        expect(receipt.transactionHash).eq(_txHash, "rceipt.txHash");
        expect(receipt.transactionIndex).eq(ZERO, "receipt.txIndex");
        expect(receipt.from).eq(ALICE.address, "receipt.from");
        expect(receipt.status).eq(ONE, "receipt.status");
    });
    it("Sender nonce increased", async () => {
        expect(await send("eth_getTransactionCount", [ALICE, "latest"])).eq(ONE);
    });
    it("Receiver nonce not increased", async () => {
        expect(await send("eth_getTransactionCount", [BOB, "latest"])).eq(ZERO);
    });
});
