import { expect } from "chai";
import { TransactionReceipt, TransactionResponse, keccak256 } from "ethers";
import { Block, Bytes } from "web3-types";

import { ALICE, BOB } from "../helpers/account";
import { BlockMode, currentBlockMode, isStratus } from "../helpers/network";
import {
    CHAIN_ID,
    CHAIN_ID_DEC,
    ETHERJS,
    HASH_ZERO,
    HEX_PATTERN,
    ONE,
    REVERSAL,
    SUCCESS,
    TEST_BALANCE,
    ZERO,
    deployTestContractBalances,
    deployTestContractBlockTimestamp,
    prepareSignedTx,
    send,
    sendAndGetError,
    sendEvmMine,
    sendExpect,
    sendGetBlockNumber,
    sendGetNonce,
    sendRawTransaction,
    sendReset,
    subscribeAndGetEvent,
    subscribeAndGetEventWithContract,
    toHex,
    toPaddedHex,
} from "../helpers/rpc";

describe("JSON-RPC", () => {
    before(() => {
        expect(currentBlockMode()).eq(BlockMode.External, "Wrong block mining mode is used");
    });

    describe("State", () => {
        it("stratus_reset", async () => {
            (await sendExpect("stratus_reset")).eq(true);
            (await sendExpect("hardhat_reset")).eq(true);
        });
    });

    describe("Metadata", () => {
        it("eth_chainId", async () => {
            (await sendExpect("eth_chainId")).eq(CHAIN_ID);
        });
        it("net_listening", async () => {
            (await sendExpect("net_listening")).to.be.true;
        });
        it("net_version", async () => {
            (await sendExpect("net_version")).eq(CHAIN_ID_DEC.toString());
        });
        it("web3_clientVersion", async () => {
            let client = await send("web3_clientVersion");
            if (isStratus) {
                expect(client).eq("stratus");
            } else {
                expect(client).to.not.be.undefined;
            }
        });
    });

    describe("Gas", () => {
        it("eth_gasPrice", async () => {
            let gasPrice = await send("eth_gasPrice");
            if (isStratus) {
                expect(gasPrice).eq(ZERO);
            } else {
                expect(gasPrice).to.match(HEX_PATTERN);
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
                await sendEvmMine();
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
        let genesisBlockHash: Bytes;
        describe("eth_getBlockByNumber", () => {
            it("fetches genesis block correctly", async () => {
                let block: Block = await send("eth_getBlockByNumber", [ZERO, true]);
                expect(block.hash).to.not.be.undefined;
                genesisBlockHash = block.hash as Bytes;
                expect(block.transactions).to.be.empty;
            });
            it("returns null if block does not exist", async () => {
                const NON_EXISTANT_BLOCK = "0xfffffff";
                let block = await send("eth_getBlockByNumber", [NON_EXISTANT_BLOCK, true]);
                expect(block).to.be.null;
            });
        });
        describe("eth_getBlockByHash", () => {
            it("fetches genesis block correctly", async () => {
                let block: Block = await send("eth_getBlockByHash", [genesisBlockHash, true]);
                expect(block.number).eq(ZERO);
                expect(block.transactions).to.be.empty;
            });
            it("returns null if block does not exist", async () => {
                let block = await send("eth_getBlockByHash", [HASH_ZERO, true]);
                expect(block).to.be.null;
            });
        });
        it("eth_getUncleByBlockHashAndIndex", async function () {
            if (isStratus) {
                (await sendExpect("eth_getUncleByBlockHashAndIndex", [ZERO, ZERO])).to.be.null;
            }
        });
    });

    describe("Logs", () => {
        describe("eth_getLogs", () => {
            it("returns the expected amount of logs for different block ranges", async () => {
                await sendReset();

                const contract = await deployTestContractBalances();
                const filter = { address: contract.target };
                await sendEvmMine();
                await contract.waitForDeployment();

                const contractOps = contract.connect(ALICE.signer());
                await sendEvmMine();
                const block = await sendGetBlockNumber();

                async function sendNTransactions(n: number) {
                    for (let i = 0; i < n; i++) {
                        await contractOps.add(ALICE.address, 10);
                    }
                    await sendEvmMine();
                }

                await sendNTransactions(8);
                await sendNTransactions(4);
                await sendNTransactions(2);
                await sendNTransactions(1);

                // single block
                expect(
                    await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block + 1), toBlock: toHex(block + 1) }]),
                ).length(8);
                expect(
                    await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block + 2), toBlock: toHex(block + 2) }]),
                ).length(4);
                expect(
                    await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block + 3), toBlock: toHex(block + 3) }]),
                ).length(2);
                expect(
                    await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block + 4), toBlock: toHex(block + 4) }]),
                ).length(1);
                expect(
                    await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block + 5), toBlock: toHex(block + 5) }]),
                ).length(0);

                // block range (offset fromBlock)
                expect(await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block + 1) }])).length(15);
                expect(await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block + 2) }])).length(7);
                expect(await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block + 3) }])).length(3);
                expect(await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block + 4) }])).length(1);
                expect(await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block + 5) }])).length(0);

                // block range (offset toBlock)
                expect(
                    await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block), toBlock: toHex(block + 1) }]),
                ).length(8);
                expect(
                    await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block), toBlock: toHex(block + 2) }]),
                ).length(12);
                expect(
                    await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block), toBlock: toHex(block + 3) }]),
                ).length(14);
                expect(
                    await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block), toBlock: toHex(block + 4) }]),
                ).length(15);
                expect(
                    await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block), toBlock: toHex(block + 4) }]),
                ).length(15);

                // block range (middle blocks)
                expect(
                    await send("eth_getLogs", [{ ...filter, fromBlock: toHex(block + 2), toBlock: toHex(block + 3) }]),
                ).length(6);
            });
        });

        describe("eth_getLogs", () => {
            it("returns no logs for queries after last mined block", async () => {
                const contract = await deployTestContractBalances();
                await sendEvmMine();
                await contract.waitForDeployment();

                const txResponse = await contract.connect(ALICE.signer()).add(ALICE.address, 10);
                await sendEvmMine();
                const txReceipt = await ETHERJS.getTransactionReceipt(txResponse.hash);
                expect(txReceipt).to.not.be.null;

                const safeTxReceipt = txReceipt as TransactionReceipt;
                expect(safeTxReceipt.status).eq(SUCCESS);

                // check log queries starting at the last mined block and starting after it
                const txBlockNumber = safeTxReceipt.blockNumber as number;
                const filter = { address: contract.target };
                expect(await send("eth_getLogs", [{ ...filter, fromBlock: toHex(txBlockNumber) }])).length(1); // last mined block

                await sendEvmMine();
                expect(await send("eth_getLogs", [{ ...filter, fromBlock: toHex(txBlockNumber + 1) }])).length(0); // 1 after mined block

                await sendEvmMine();
                expect(await send("eth_getLogs", [{ ...filter, fromBlock: toHex(txBlockNumber + 2) }])).length(0); // 2 after mined block
            });
        });
    });

    describe("Transaction", () => {
        describe("eth_getTransactionByHash", () => {
            it("returns transaction before and after minting", async () => {
                const amount = 1;
                const nonce = await sendGetNonce(ALICE);

                const signedTx = await ALICE.signWeiTransfer(BOB.address, amount, nonce);
                const txHash = keccak256(signedTx);
                await sendRawTransaction(signedTx);
                const txResponseAfterSending: TransactionResponse | null = await ETHERJS.getTransaction(txHash);
                expect(txResponseAfterSending).to.not.be.null;

                await sendEvmMine();
                const txResponseAfterMinting: TransactionResponse | null = await ETHERJS.getTransaction(txHash);
                expect(txResponseAfterMinting).to.not.be.null;

                expect(txResponseAfterSending?.hash).eq(txHash);
                expect(txResponseAfterMinting?.hash).eq(txHash);
            });
        });

        describe("eth_sendRawTransaction", () => {
            it("Returns an expected result when a contract transaction fails", async () => {
                // deploy
                const contract = await deployTestContractBalances();
                await sendEvmMine();
                contract.waitForDeployment();

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
                const txReceiptAfterMining = await ETHERJS.getTransactionReceipt(expectedTxHash);
                expect(txReceiptAfterMining).to.not.be.null;
                expect(txReceiptAfterMining?.status).eq(REVERSAL);
                expect(actualTxHash).eq(expectedTxHash);
            });
        });
    });

    describe("Call", () => {
        describe("eth_call", () => {
            it("Returns an expected result when sending calls", async () => {
                // deploy
                const contract = await deployTestContractBalances();
                await sendEvmMine();
                contract.waitForDeployment();

                {
                    // eth_call should not really change the state
                    const data = contract.interface.encodeFunctionData("add", [ALICE.address, 5]);
                    const transaction = { to: contract.target, data: data };
                    await send("eth_call", [transaction, "latest"]);
                    await sendEvmMine();
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
                await sendEvmMine();
                contract.waitForDeployment();

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

        describe("Block timestamp", () => {
            it("transaction executes with pending block timestamp", async () => {
                await sendReset();
        
                const contract = await deployTestContractBlockTimestamp();
                
                await sendEvmMine();
        
                // Get current time before transaction
                const beforeTx = Math.floor(Date.now() / 1000);
        
                // Record timestamp in contract
                const tx = await contract.recordTimestamp();
        
                // Mine block to include the transaction
                await sendEvmMine();
        
                const receipt = await tx.wait();
        
                // Get the timestamp from contract event
                const event = receipt.logs[0];
                const recordedTimestamp = contract.interface.parseLog({
                    topics: event.topics,
                    data: event.data,
                })?.args.timestamp;
        
                // Get the block timestamp
                const block = await ETHERJS.getBlock(receipt.blockNumber);
                const blockTimestamp = block!.timestamp;
        
                // Validate contract saw same timestamp as block
                expect(recordedTimestamp).to.equal(blockTimestamp);
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
                const automaticallySendEVMMines = true;
                const response = await subscribeAndGetEventWithContract(
                    "logs",
                    waitTimeInMilliseconds,
                    2,
                    automaticallySendEVMMines,
                );
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
                const automaticallySendEVMMines = true;
                const response = await subscribeAndGetEventWithContract(
                    "newPendingTransactions",
                    waitTimeInMilliseconds,
                    2,
                    automaticallySendEVMMines,
                );
                expect(response).to.not.be.undefined;

                const params = response.params;
                expect(params).to.have.property("subscription").that.is.a("string");
                expect(params).to.have.property("result").that.is.an("string");
            });
        });
    });
});
