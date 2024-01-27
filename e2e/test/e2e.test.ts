import { expect } from "chai";
import { keccak256 } from "ethers";
import { match } from "ts-pattern";
import { Block, Transaction, TransactionReceipt } from "web3-types";
import { contract } from "web3/lib/commonjs/eth.exports";

import { TestContract } from "../typechain-types";
import { ACCOUNTS, ALICE, BOB, CHARLIE } from "./helpers/account";
import { CURRENT_NETWORK, Network } from "./helpers/network";
import * as rpc from "./helpers/rpc";

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

const CHAIN_ID_DEC = 2008;
const CHAIN_ID = rpc.toHex(CHAIN_ID_DEC);

// Special numbers
const ZERO = "0x0";
const ONE = "0x1";

// Special hashes
const HASH_EMPTY_UNCLES = "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347";
const HASH_EMPTY_TRANSACTIONS = "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421";

// Test contract topics
const CONTRACT_TOPIC_ADD = "0x2728c9d3205d667bbc0eefdfeda366261b4d021949630c047f3e5834b30611ab";
const CONTRACT_TOPIC_SUB = "0xf9c652bcdb0eed6299c6a878897eb3af110dbb265833e7af75ad3d2c2f4a980c";

// -----------------------------------------------------------------------------
// RPC tests
// -----------------------------------------------------------------------------
describe("JSON-RPC methods", () => {
    describe("Metadata", () => {
        it("eth_chainId", async () => {
            (await rpc.sendExpect("eth_chainId")).eq(CHAIN_ID);
        });
        it("net_version", async () => {
            (await rpc.sendExpect("net_version")).eq(CHAIN_ID_DEC + "");
        });
        it("web3_clientVersion", async () => {
            let client = await rpc.sendExpect("web3_clientVersion");
            match(CURRENT_NETWORK).with(Network.Stratus, () => client.deep.eq("stratus"));
        });
    });

    describe("Gas", () => {
        it("eth_gasPrice", async () => {
            let gasPrice = await rpc.sendExpect("eth_gasPrice");
            match(CURRENT_NETWORK)
                .with(Network.Stratus, () => gasPrice.eq(ZERO))
                .otherwise(() => gasPrice.not.eq(ZERO));
        });
    });

    describe("Account", () => {
        it("eth_getTransactionCount", async () => {
            (await rpc.sendExpect("eth_getTransactionCount", [ALICE])).eq(ZERO);
            (await rpc.sendExpect("eth_getTransactionCount", [ALICE, "latest"])).eq(ZERO);
        });
    });

    describe("Block", () => {
        it("eth_blockNumber", async function () {
            (await rpc.sendExpect("eth_blockNumber")).eq(ZERO);
        });
        it("eth_getBlockByNumber", async function () {
            let block: Block = await rpc.send("eth_getBlockByNumber", [ZERO, true]);
            expect(block.transactions.length).eq(0);
        });
        it("eth_getUncleByBlockHashAndIndex", async function () {
            let block: Block = await rpc.send("eth_getUncleByBlockHashAndIndex", [ZERO, ZERO]);
            expect(block).eq(null);
        });
    });
});

// -----------------------------------------------------------------------------
// Native transfer tests
// -----------------------------------------------------------------------------
describe("Transaction: wei transfer", () => {
    var _tx: Transaction;
    var _txHash: string;
    var _block: Block;

    it("Send transaction", async () => {
        let txSigned = await ALICE.signer().signTransaction({
            chainId: CHAIN_ID_DEC,
            to: BOB,
            value: 0,
            gasPrice: 0,
            gasLimit: 500_000,
        });
        _txHash = await rpc.send("eth_sendRawTransaction", [txSigned]);
        expect(_txHash).eq(keccak256(txSigned));
    });
    it("Transaction is created", async () => {
        _tx = await rpc.send("eth_getTransactionByHash", [_txHash]);
        expect(_tx.from).eq(ALICE.address, "tx.from");
        expect(_tx.to).eq(BOB.address, "tx.to");
        expect(_tx.nonce).eq(ZERO, "tx.nonce");
        expect(_tx.chainId).eq(CHAIN_ID, "tx.chainId");
    });
    it("Block is created", async () => {
        expect(await rpc.send("eth_blockNumber")).eq(ONE);

        _block = await rpc.send("eth_getBlockByNumber", [ONE, true]);
        expect(_block.number).eq(ONE);

        expect(_block.transactionsRoot).not.eq(HASH_EMPTY_TRANSACTIONS);
        expect(_block.uncles).lengthOf(0);
        expect(_block.sha3Uncles).eq(HASH_EMPTY_UNCLES);

        expect(_block.transactions.length).eq(1);
        expect(_block.transactions[0] as Transaction).deep.eq(_tx);
    });
    it("Receipt is created", async () => {
        let receipt: TransactionReceipt = await rpc.send("eth_getTransactionReceipt", [_txHash]);
        expect(receipt.blockNumber).eq(_block.number, "receipt.blockNumber");
        expect(receipt.blockHash).eq(_block.hash, "receipt.blockHash");
        expect(receipt.transactionHash).eq(_txHash, "rceipt.txHash");
        expect(receipt.transactionIndex).eq(ZERO, "receipt.txIndex");
        expect(receipt.from).eq(ALICE.address, "receipt.from");
        expect(receipt.status).eq(ONE, "receipt.status");
    });
    it("Sender nonce increased", async () => {
        expect(await rpc.send("eth_getTransactionCount", [ALICE, "latest"])).eq(ONE);
    });
    it("Receiver nonce not increased", async () => {
        expect(await rpc.send("eth_getTransactionCount", [BOB, "latest"])).eq(ZERO);
    });
});

// -----------------------------------------------------------------------------
// Contracts tests
// -----------------------------------------------------------------------------
describe("Transaction: serial requests", () => {
    var _contract: TestContract;
    var _block: number;

    it("Contract is deployed", async () => {
        _contract = await rpc.deployTestContract();
    });

    it("Performs add and sub", async () => {
        _block = await rpc.getBlockNumber();

        // initial balance
        expect(await _contract.get(CHARLIE.address)).eq(0);

        // mutations
        let nonce = await rpc.getNonce(CHARLIE.address);
        const contractOps = _contract.connect(CHARLIE.signer());
        await contractOps.add(CHARLIE.address, 10, { nonce: nonce++ });
        await contractOps.add(CHARLIE.address, 15, { nonce: nonce++ });
        await contractOps.sub(CHARLIE.address, 5, { nonce: nonce++ });

        // final balance
        expect(await _contract.get(CHARLIE.address)).eq(20);
    });

    it("Generates logs", async () => {
        let filter = { address: _contract.target, fromBlock: rpc.toHex(_block + 0) };

        // filter fromBlock
        (await rpc.sendExpect("eth_getLogs", [{ ...filter, fromBlock: rpc.toHex(_block + 0) }])).length(3);
        (await rpc.sendExpect("eth_getLogs", [{ ...filter, fromBlock: rpc.toHex(_block + 1) }])).length(3);
        (await rpc.sendExpect("eth_getLogs", [{ ...filter, fromBlock: rpc.toHex(_block + 2) }])).length(2);
        (await rpc.sendExpect("eth_getLogs", [{ ...filter, fromBlock: rpc.toHex(_block + 3) }])).length(1);
        (await rpc.sendExpect("eth_getLogs", [{ ...filter, fromBlock: rpc.toHex(_block + 4) }])).length(1);

        // filter topics
        (await rpc.sendExpect("eth_getLogs", [{ ...filter, topics: [CONTRACT_TOPIC_ADD] }])).length(2);
        (await rpc.sendExpect("eth_getLogs", [{ ...filter, topics: [CONTRACT_TOPIC_SUB] }])).length(1);
    });
});

// -----------------------------------------------------------------------------
// Parallel transactions tests
// -----------------------------------------------------------------------------
describe("Transaction: parallel requests", async () => {
    var _contract: TestContract;
    it("Deploy test contract", async () => {
        _contract = await rpc.deployTestContract();
    });

    it("Sends parallel transactions", async () => {
        // initial balance
        expect(await _contract.get(CHARLIE.address)).eq(0);

        // prepare transactions
        const transactions = [];
        var expectedBalance = 0;
        for (let accountIndex = 0; accountIndex < ACCOUNTS.length; accountIndex++) {
            const account = ACCOUNTS[accountIndex];
            const amount = accountIndex + 1;
            expectedBalance += amount;

            const nonce = await rpc.getNonce(account.address);
            const tx = await _contract
                .connect(account.signer())
                .add.populateTransaction(CHARLIE.address, amount, { nonce: nonce, gasPrice: 0 });
            const txSigned = await account.signer().signTransaction(tx);
            transactions.push(txSigned);
        }

        // send all transactions in parallel
        const requests = [];
        for (const tx of transactions) {
            const req = rpc.send("eth_sendRawTransaction", [tx]).then((_) => {});
            requests.push(req);
        }
        await Promise.all(requests);

        // final balance
        // expect(await _contract.get(CHARLIE.address)).eq(expectedBalance);
    });
});
