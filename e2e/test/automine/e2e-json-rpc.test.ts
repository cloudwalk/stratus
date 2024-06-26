import { expect } from "chai";
import { keccak256 } from "ethers";
import { Block, Bytes, TransactionReceipt } from "web3-types";

import { ALICE, BOB } from "../helpers/account";
import { isStratus } from "../helpers/network";
import {
    CHAIN_ID,
    CHAIN_ID_DEC,
    ETHERJS,
    HASH_ZERO,
    HEX_PATTERN,
    ONE,
    TEST_BALANCE,
    ZERO,
    deployTestContractBalances,
    prepareSignedTx,
    send,
    sendAndGetError,
    sendEvmMine,
    sendExpect,
    sendRawTransaction,
    sendReset,
    subscribeAndGetEvent,
    subscribeAndGetEventWithContract,
    toHex,
} from "../helpers/rpc";

describe("JSON-RPC", () => {
    describe("State", () => {
        it("debug_setHead / hardhat_reset", async () => {
            if (isStratus) {
                (await sendExpect("debug_setHead", [ZERO])).eq(ZERO);
            } else {
                (await sendExpect("hardhat_reset", [])).eq(true);
            }
        });
    });

    describe("Metadata", () => {
        it("eth_chainId", async () => {
            (await sendExpect("eth_chainId")).eq(CHAIN_ID);
        });
        it("net_listening", async () => {
            (await sendExpect("net_listening")).eq(true);
        });
        it("net_version", async () => {
            (await sendExpect("net_version")).eq(CHAIN_ID_DEC + "");
        });
        it("web3_clientVersion", async () => {
            let client = await sendExpect("web3_clientVersion");
            if (isStratus) {
                client.eq("stratus");
            }
        });
    });

    describe("Gas", () => {
        it("eth_gasPrice", async () => {
            let gasPrice = await sendExpect("eth_gasPrice");
            if (isStratus) {
                gasPrice.eq(ZERO);
            } else {
                gasPrice.not.eq(ZERO);
            }
        });
        it("eth_estimateGas", async () => {
            let tx = { from: ALICE.address, to: BOB.address, value: "0x1" };
            let gas = await send("eth_estimateGas", [tx]);
            expect(gas).match(HEX_PATTERN, "format");

            const gasDec = parseInt(gas, 16);
            expect(gasDec).to.be.greaterThan(0).and.lessThan(1_000_000);
        });
    });

    describe("Account", () => {
        it("eth_getTransactionCount", async () => {
            (await sendExpect("eth_getTransactionCount", [ALICE])).eq(ZERO);
            (await sendExpect("eth_getTransactionCount", [ALICE, "latest"])).eq(ZERO);
            (await sendExpect("eth_getTransactionCount", [ALICE, "pending"])).eq(ZERO);
        });
        it("eth_getBalance", async () => {
            (await sendExpect("eth_getBalance", [ALICE])).eq(TEST_BALANCE);
            (await sendExpect("eth_getBalance", [ALICE, "latest"])).eq(TEST_BALANCE);
        });
        it("eth_getCode", () => {
            it("code for non-existent contract is 0x", async () => {
                (await sendExpect("eth_getCode", [ALICE.address, "latest"])).eq("0x");
            });
        });
    });

    describe("Block", () => {
        it("eth_blockNumber", async function () {
            (await sendExpect("eth_blockNumber")).eq(ZERO);
            await sendEvmMine();
            (await sendExpect("eth_blockNumber")).eq(ONE);
            await sendReset();
        });
        let hash: Bytes;
        describe("eth_getBlockByNumber", () => {
            it("fetches genesis block correctly", async () => {
                let block: Block = await send("eth_getBlockByNumber", [ZERO, true]);
                expect(block.hash).to.not.be.undefined;
                hash = block.hash as Bytes; // get the genesis hash to use on the next test
                expect(block.transactions.length).eq(0);
            });
            it("returns null if block does not exist", async () => {
                const NON_EXISTANT_BLOCK = "0xfffffff";
                let block = await send("eth_getBlockByNumber", [NON_EXISTANT_BLOCK, true]);
                expect(block).to.be.null;
            });
        });
        describe("eth_getBlockByHash", () => {
            it("fetches genesis block correctly", async () => {
                let block: Block = await send("eth_getBlockByHash", [hash, true]);
                expect(block.number).eq("0x0");
                expect(block.transactions.length).eq(0);
            });
            it("returns null if block does not exist", async () => {
                let block = await send("eth_getBlockByHash", [HASH_ZERO, true]);
                expect(block).to.be.null;
            });
        });
        it("eth_getUncleByBlockHashAndIndex", async function () {
            if (isStratus) {
                (await sendExpect("eth_getUncleByBlockHashAndIndex", [ZERO, ZERO])).eq(null);
            }
        });
    });

    describe("Logs", () => {
        describe("eth_getLogs", () => {
            it("returns no logs for queries after last mined block", async () => {
                // mine a test transaction
                const contract = await deployTestContractBalances();
                const txResponse = await contract.connect(ALICE.signer()).add(ALICE.address, 10);
                const txReceipt = await ETHERJS.getTransactionReceipt(txResponse.hash);
                expect(txReceipt).exist;
                expect(txReceipt?.status).eq(1);

                // check log queries starting at the last mined block and starting after it
                const txBlockNumber = txReceipt?.blockNumber ?? 0;
                const filter = { address: contract.target };
                expect(await send("eth_getLogs", [{ ...filter, fromBlock: toHex(txBlockNumber) }])).length(1); // last mined block
                expect(await send("eth_getLogs", [{ ...filter, fromBlock: toHex(txBlockNumber + 1) }])).length(0); // 1 after mined block
                expect(await send("eth_getLogs", [{ ...filter, fromBlock: toHex(txBlockNumber + 2) }])).length(0); // 2 after mined block
            });
        });
    });

    describe("Transaction", () => {
        describe("eth_sendRawTransaction", () => {
            it("Returns an expected result when a contract transaction fails", async () => {
                // deploy
                const contract = await deployTestContractBalances();
                await sendEvmMine();

                // send a transaction that will fail
                const signedTx = await prepareSignedTx({
                    contract,
                    account: ALICE,
                    methodName: "sub",
                    methodParameters: [ALICE.address, 1],
                });
                const expectedTxHash = keccak256(signedTx);
                const actualTxHash = await sendRawTransaction(signedTx);
                await sendEvmMine();

                // validate
                const txReceipt = await ETHERJS.getTransactionReceipt(expectedTxHash);
                expect(txReceipt).exist;
                expect(txReceipt?.status).eq(0); // The transaction really failed
                expect(actualTxHash).eq(expectedTxHash); // The transaction sending function returned the expected result
            });
        });
    });

    describe("Evm", () => {
        async function latest(): Promise<{ timestamp: number; block_number: number }> {
            const block = await send("eth_getBlockByNumber", ["latest", false]);
            return { timestamp: parseInt(block.timestamp, 16), block_number: parseInt(block.number, 16) };
        }

        it("evm_mine", async () => {
            let prev_number = (await latest()).block_number;
            await sendEvmMine();
            expect((await latest()).block_number).eq(prev_number + 1);
        });

        describe("evm_setNextBlockTimestamp", () => {
            let target = Math.floor(Date.now() / 1000) + 10;
            it("sets the next block timestamp", async () => {
                await send("evm_setNextBlockTimestamp", [target]);
                await sendEvmMine();
                expect((await latest()).timestamp).eq(target);
            });

            it("offsets subsequent timestamps", async () => {
                await new Promise((resolve) => setTimeout(resolve, 1000));
                await sendEvmMine();
                expect((await latest()).timestamp).to.be.greaterThan(target);
            });

            it("resets the changes when sending 0", async () => {
                await send("evm_setNextBlockTimestamp", [0]);
                let mined_timestamp = Math.floor(Date.now() / 1000);
                await sendEvmMine();
                let latest_timestamp = (await latest()).timestamp;
                expect(latest_timestamp)
                    .gte(mined_timestamp)
                    .lte(Math.floor(Date.now() / 1000));
            });

            it("handle negative offsets", async () => {
                const past = Math.floor(Date.now() / 1000);
                await new Promise((resolve) => setTimeout(resolve, 2000));
                await send("evm_setNextBlockTimestamp", [past]);
                await sendEvmMine();
                expect((await latest()).timestamp).eq(past);
                await new Promise((resolve) => setTimeout(resolve, 1000));
                await sendEvmMine();
                expect((await latest()).timestamp)
                    .to.be.greaterThan(past)
                    .lessThan(Math.floor(Date.now() / 1000));

                await send("evm_setNextBlockTimestamp", [0]);
            });
        });
    });

    describe("Subscription", () => {
        describe("eth_subscribe", () => {
            it("fails on HTTP", async () => {
                const error = await sendAndGetError("eth_subscribe", ["newHeads"]);
                expect(error).to.not.be.null;
                expect(error.code).eq(-32603); // Internal error
            });
            it("subscribes to newHeads receives success subscription event", async () => {
                const waitTimeInMilliseconds = 40;
                const response = await subscribeAndGetEvent("newHeads", waitTimeInMilliseconds);
                expect(response).to.not.be.undefined;
                expect(response.id).to.not.be.undefined;
                expect(response.result).to.not.be.undefined;
            });

            it("subscribes to logs receives success subscription event", async () => {
                const waitTimeInMilliseconds = 40;
                const response = await subscribeAndGetEvent("logs", waitTimeInMilliseconds);
                expect(response).to.not.be.undefined;
                expect(response.id).to.not.be.undefined;
                expect(response.result).to.not.be.undefined;
            });

            it("subscribes to newPendingTransactions receives success subscription event", async () => {
                const waitTimeInMilliseconds = 40;
                const response = await subscribeAndGetEvent("newPendingTransactions", waitTimeInMilliseconds);
                expect(response).to.not.be.undefined;
                expect(response.id).to.not.be.undefined;
                expect(response.result).to.not.be.undefined;
            });

            it("subscribes to unsupported receives error subscription event", async () => {
                const waitTimeInMilliseconds = 40;
                const response = await subscribeAndGetEvent("unsupportedSubscription", waitTimeInMilliseconds);
                expect(response).to.not.be.undefined;
                expect(response.id).to.not.be.undefined;
                expect(response.error).to.not.be.undefined;
                expect(response.error.code).to.not.be.undefined;
                expect(response.error.code).to.be.a("number");
                expect(response.error.code).eq(-32602);
            });

            it("validates newHeads event", async () => {
                const waitTimeInMilliseconds = 40;
                const response = await subscribeAndGetEvent("newHeads", waitTimeInMilliseconds, 2);
                expect(response).to.not.be.undefined;

                const params = response.params;
                expect(params).to.have.property("subscription").that.is.a("string");
                expect(params).to.have.property("result").that.is.an("object");

                const result = params.result;
                expect(result).to.have.property("hash").that.is.a("string");
                expect(result).to.have.property("parentHash").that.is.a("string");
                expect(result).to.have.property("sha3Uncles").that.is.a("string");
                expect(result).to.have.property("miner").that.is.a("string");
                expect(result).to.have.property("stateRoot").that.is.a("string");
                expect(result).to.have.property("transactionsRoot").that.is.a("string");
                expect(result).to.have.property("receiptsRoot").that.is.a("string");
                expect(result).to.have.property("number").that.is.a("string");
                expect(result).to.have.property("gasUsed").that.is.a("string");
                expect(result).to.have.property("extraData").that.is.a("string");
                expect(result).to.have.property("logsBloom").that.is.a("string");
                expect(result).to.have.property("timestamp").that.is.a("string");
                expect(result).to.have.property("difficulty").that.is.a("string");
                expect(result).to.have.property("totalDifficulty").that.is.a("string");
                expect(result).to.have.property("uncles").that.is.an("array");
                expect(result).to.have.property("transactions").that.is.an("array");
                expect(result).to.have.property("size");
                expect(result).to.have.property("mixHash");
                expect(result).to.have.property("nonce").that.is.a("string");
                expect(result).to.have.property("baseFeePerGas").that.is.a("string");
            });

            it("validates logs event", async () => {
                await sendReset();
                const waitTimeInMilliseconds = 40;
                const response = await subscribeAndGetEventWithContract("logs", waitTimeInMilliseconds, 2);
                expect(response).to.not.be.undefined;

                const params = response.params;
                expect(params).to.have.property("subscription").that.is.a("string");
                expect(params).to.have.property("result").that.is.an("object");

                const result = params.result;
                expect(result).to.have.property("address").that.is.a("string");
                expect(result).to.have.property("topics").that.is.an("array");
                expect(result).to.have.property("data").that.is.a("string");
                expect(result).to.have.property("blockHash").that.is.a("string");
                expect(result).to.have.property("blockNumber").that.is.a("string");
                expect(result).to.have.property("transactionHash").that.is.a("string");
                expect(result).to.have.property("transactionIndex").that.is.a("string");
                expect(result).to.have.property("logIndex").that.is.a("string");
                expect(result).to.have.property("removed").that.is.a("boolean");
            });

            it("validates newPendingTransactions event", async () => {
                await sendReset();
                const waitTimeInMilliseconds = 40;
                const response = await subscribeAndGetEventWithContract(
                    "newPendingTransactions",
                    waitTimeInMilliseconds,
                    2,
                );
                expect(response).to.not.be.undefined;

                const params = response.params;
                expect(params).to.have.property("subscription").that.is.a("string");
                expect(params).to.have.property("result").that.is.an("string");
            });
        });
    });
});
