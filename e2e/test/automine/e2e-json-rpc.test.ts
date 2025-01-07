import { expect } from "chai";
import { keccak256 } from "ethers";
import { JsonRpcProvider } from "ethers";
import { Block, Bytes } from "web3-types";

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
    deployTestContractBlockTimestamp,
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
    toPaddedHex,
} from "../helpers/rpc";

describe("JSON-RPC", () => {
    describe("State", () => {
        it("reset", async () => {
            if (isStratus) {
                (await sendExpect("stratus_reset")).eq(true);
            } else {
                (await sendExpect("hardhat_reset")).eq(true);
            }
        });
    });

    describe("Unknown Clients", () => {
        before(async () => {
            if (isStratus) {
                // Ensure clean state
                await send("stratus_enableUnknownClients");
            }
        });

        it("disabling unknown clients blocks requests without client identification", async function () {
            if (!isStratus) {
                this.skip();
                return;
            }

            await send("stratus_disableUnknownClients");

            // Request without client identification should fail
            const error = await sendAndGetError("eth_blockNumber");
            expect(error.code).eq(1003);

            // GET request to health endpoint should fail when unknown clients are disallowed
            const healthResponseErr = await fetch("http://localhost:3000/health");
            expect(healthResponseErr.status).eq(500);

            // Requests with client identification should succeed
            const validHeaders = {
                "x-app": "test-client",
                "x-stratus-app": "test-client",
                "x-client": "test-client",
                "x-stratus-client": "test-client",
            };

            for (const [header, value] of Object.entries(validHeaders)) {
                const headers = { [header]: value };
                const result = await send("eth_blockNumber", [], headers);
                expect(result).to.match(/^0x[0-9a-f]+$/);
            }

            // URL parameters should also work
            const validUrlParams = ["app=test-client", "client=test-client"];
            const healthResponse = await fetch(`http://localhost:3000/health?app=test-client`);
            expect(healthResponse.status).eq(200);
            for (const param of validUrlParams) {
                const providerWithParam = new JsonRpcProvider(`http://localhost:3000?${param}`);
                const blockNumber = await providerWithParam.getBlockNumber();
                expect(blockNumber).to.be.a("number");
            }
        });

        it("enabling unknown clients allows all requests", async function () {
            if (!isStratus) {
                this.skip();
                return;
            }

            await send("stratus_disableUnknownClients");
            await send("stratus_enableUnknownClients");

            // Request without client identification should now succeed
            const blockNumber = await send("eth_blockNumber");
            expect(blockNumber).to.match(/^0x[0-9a-f]+$/);
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
        describe("eth_getCode", () => {
            it("contract code is available in the block it was deployed", async () => {
                await sendReset();
                const contract = await deployTestContractBalances();
                (await sendExpect("eth_getCode", [contract.target, "latest"])).not.eq("0x");
                (await sendExpect("eth_getCode", [contract.target, "0x1"])).not.eq("0x");
            });
            it("code for non-existent contract is 0x", async () => {
                (await sendExpect("eth_getCode", [ALICE.address, "latest"])).eq("0x");
            });
        });
    });

    describe("Block", () => {
        it("eth_blockNumber", async function () {
            await sendReset();
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

                // send a transaction that will fail
                const signedTx = await prepareSignedTx({
                    contract,
                    account: ALICE,
                    methodName: "sub",
                    methodParameters: [ALICE.address, 1],
                });
                const expectedTxHash = keccak256(signedTx);
                const actualTxHash = await sendRawTransaction(signedTx);

                // validate
                const txReceiptAfterMining = await ETHERJS.getTransactionReceipt(expectedTxHash);
                expect(txReceiptAfterMining).exist;
                expect(txReceiptAfterMining?.status).eq(0);
                expect(actualTxHash).eq(expectedTxHash);
            });
        });
    });

    describe("Call", () => {
        describe("eth_call", () => {
            it("Returns an expected result when sending calls", async () => {
                // deploy
                const contract = await deployTestContractBalances();

                {
                    const data = contract.interface.encodeFunctionData("add", [ALICE.address, 5]);
                    const transaction = { to: contract.target, data: data };
                    await send("eth_call", [transaction, "latest"]);
                }

                const data = contract.interface.encodeFunctionData("get", [ALICE.address]);
                const transaction = { to: contract.target, data: data };
                const currentAliceBalance = await send("eth_call", [transaction, "latest"]);

                // validate
                const expectedAliceBalance = toPaddedHex(0, 32);
                expect(currentAliceBalance).eq(expectedAliceBalance);
            });

            it("Works when using the field 'input' instead of 'data'", async () => {
                // deploy
                const contract = await deployTestContractBalances();

                const data = contract.interface.encodeFunctionData("get", [ALICE.address]);
                const transaction = { to: contract.target, input: data };
                const currentAliceBalance = await send("eth_call", [transaction, "latest"]);

                // validate
                const expectedAliceBalance = toPaddedHex(0, 32);
                expect(currentAliceBalance).eq(expectedAliceBalance);
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
            let initialTarget: number;

            beforeEach(async () => {
                await send("evm_setNextBlockTimestamp", [0]);
            });

            it("sets the next block timestamp", async () => {
                initialTarget = Math.floor(Date.now() / 1000) + 100;
                await send("evm_setNextBlockTimestamp", [initialTarget]);
                await sendEvmMine();
                expect((await latest()).timestamp).eq(initialTarget);
            });

            it("offsets subsequent timestamps", async () => {
                const target = Math.floor(Date.now() / 1000) + 100;
                await send("evm_setNextBlockTimestamp", [target]);
                await sendEvmMine();

                await new Promise((resolve) => setTimeout(resolve, 1000));
                await sendEvmMine();
                expect((await latest()).timestamp).to.be.greaterThan(target);
            });

            it("resets the changes when sending 0", async () => {
                const currentTimestamp = (await latest()).timestamp;
                await send("evm_setNextBlockTimestamp", [0]);
                await sendEvmMine();
                const newTimestamp = (await latest()).timestamp;
                expect(newTimestamp).to.be.greaterThan(currentTimestamp);
            });

            it("handle negative offsets", async () => {
                const currentBlock = await latest();
                const futureTimestamp = currentBlock.timestamp + 100;

                await send("evm_setNextBlockTimestamp", [futureTimestamp]);
                await sendEvmMine();

                const pastTimestamp = currentBlock.timestamp - 100;
                await send("evm_setNextBlockTimestamp", [pastTimestamp]);
                await sendEvmMine();

                const newTimestamp = (await latest()).timestamp;
                expect(newTimestamp).to.be.greaterThan(futureTimestamp);
            });
        });

        describe("Block timestamp", () => {
            it("transaction executes with pending block timestamp", async () => {
                await sendReset();
                const contract = await deployTestContractBlockTimestamp();

                // Get initial timestamp
                const initialTimestamp = await contract.getCurrentTimestamp();
                expect(initialTimestamp).to.be.gt(0);

                // Record timestamp in contract
                const tx = await contract.recordTimestamp();
                const receipt: TransactionReceipt = await tx.wait();

                // Get the timestamp from contract event
                const event = receipt.logs[0];
                const recordedTimestamp = contract.interface.parseLog({
                    topics: event.topics,
                    data: event.data,
                })?.args.timestamp;

                // Get the block timestamp
                const block = await ETHERJS.getBlock(receipt.blockNumber);
                const blockTimestamp = block!.timestamp;

                // Get stored record from contract
                const records = await contract.getRecords();
                expect(records.length).to.equal(1);

                // Validate timestamps match across all sources
                expect(recordedTimestamp).to.equal(blockTimestamp);
                expect(records[0].timestamp).to.equal(recordedTimestamp);
                expect(records[0].blockNumber).to.equal(receipt.blockNumber);

                // Verify that time is advancing
                const finalTimestamp = await contract.getCurrentTimestamp();
                expect(finalTimestamp).to.be.gt(initialTimestamp);
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
                expect(response.error.code).eq(1006);
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
