import { expect } from "chai";
import { keccak256 } from "ethers";
import { Block, Transaction, TransactionReceipt } from "web3-types";

import { ALICE, Account, BOB, randomAccounts } from "./helpers/account";
import { isStratus } from "./helpers/network";
import {
    CHAIN_ID,
    CHAIN_ID_DEC,
    HASH_EMPTY_TRANSACTIONS,
    HASH_EMPTY_UNCLES,
    ONE,
    TEST_BALANCE,
    TEST_TRANSFER,
    ZERO,
    fromHexTimestamp,
    send,
    sendGetBalance,
    sendRawTransaction,
    sendReset,
} from "./helpers/rpc";

describe("GAS_LIMIT", () => {
    var _tx: Transaction;
    var _txHash: string;
    var _block: Block;
    var _txSentTimestamp: number;
    var new_account: Account;

    it("Resets blockchain", async () => {
        await sendReset();
    });
    it("Send transaction", async () => {
        let txSigned = await ALICE.signWeiTransfer(BOB.address, TEST_TRANSFER, 1);
        _txSentTimestamp = Math.floor(Date.now() / 1000);
        _txHash = await sendRawTransaction(txSigned);
        expect(_txHash).eq(keccak256(txSigned));
    });
    it("Block is created", async () => {
        expect(await send("eth_blockNumber")).eq(ONE);

        _block = await send("eth_getBlockByNumber", [ONE, true]);
        expect(_block.number).eq(ONE);
    });
    it("Send transaction to new account", async () => {
        new_account = randomAccounts(1)[0];
        let txSigned = await ALICE.signWeiTransfer(new_account.address, TEST_TRANSFER, 1_000_000, 1);
        _txSentTimestamp = Math.floor(Date.now() / 1000);
        _txHash = await sendRawTransaction(txSigned);
    });
    it("Receiver balance is increased", async () => {
        expect(await sendGetBalance(new_account)).eq(TEST_TRANSFER);
    });
});
