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
    deployTestRevertReason,
    prepareSignedTx,
    send,
    sendAndGetError,
    sendAndGetFullResponse,
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
        it("reset", async () => {
            if (isStratus) {
                (await sendExpect("stratus_reset")).eq(true);
            } else {
                (await sendExpect("hardhat_reset")).eq(true);
            }
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
        describe("stratus_getBlockAndReceipts", () => {
            it("fetches block with receipt", async () => {
                await sendReset();

                // Send a transaction
                const amount = 1;
                const nonce = await sendGetNonce(ALICE);
                const signedTx = await ALICE.signWeiTransfer(BOB.address, amount, nonce);
                const txHash = keccak256(signedTx);
                await sendRawTransaction(signedTx);
                await sendEvmMine();

                // Get block number and hash
                const blockNumber = await send("eth_blockNumber");
                const block = await send("eth_getBlockByNumber", [blockNumber, true]);
                const blockHash = block.hash;

                // Get individual block and receipt
                const individualBlock = await send("eth_getBlockByHash", [blockHash, true]);
                const individualReceipt = await send("eth_getTransactionReceipt", [txHash, true]);

                // Get block and receipts using stratus endpoint
                const response = await send("stratus_getBlockAndReceipts", [blockHash]);

                // Validate block
                expect(response.block).to.not.be.null;
                expect(response.block).to.deep.equal(individualBlock);

                // Validate receipt
                expect(response.receipts).to.have.length(1);
                const combinedReceipt = response.receipts[0];
                const safeIndividualReceipt = individualReceipt!;

                // Compare receipt fields
                expect(combinedReceipt).to.deep.equal(safeIndividualReceipt);
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
            
            it("Returns an error when nonce is too high", async () => {
                // get current nonce
                const currentNonce = await sendGetNonce(ALICE);

                // prepare transaction with nonce that's too high
                const tooHighNonce = currentNonce + 1;
                const signedTx = await ALICE.signWeiTransfer(BOB.address, 1, tooHighNonce);

                // send transaction and check error
                const error = await sendAndGetError("eth_sendRawTransaction", [signedTx]);
                expect(error).to.be.an("object");
                expect(error.code).to.equal(2002);
                expect(error.message).to.equal(`Transaction nonce ${tooHighNonce} does not match account nonce ${currentNonce}.`);
            });
        });

        describe("debug_traceTransaction", () => {
            it("callTracer", async () => {
                const contract = await deployTestRevertReason();
                await sendEvmMine();
                await contract.waitForDeployment();

                const signedTx = await prepareSignedTx({
                    contract,
                    account: ALICE,
                    methodName: "revertWithKnownError",
                    methodParameters: [],
                });
                const txHash = await sendRawTransaction(signedTx);
                await sendEvmMine();
                const txReceipt = await ETHERJS.getTransactionReceipt(txHash);

                const trace = await sendAndGetFullResponse("debug_traceTransaction", [
                    txReceipt?.hash,
                    { tracer: "callTracer" },
                ]);

                expect(trace.data.result.from).to.eq("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266");
                expect(trace.data.result.input).to.eq("0x2b3d7bd2");
                expect(trace.data.result.output).to.eq("0x22aa4404");
                expect(trace.data.result.error).to.eq("execution reverted");
                expect(trace.data.result.value).to.eq("0x0");
                expect(trace.data.result.type).to.eq("CALL");
            });

            it("4byteTracer", async () => {
                const contract = await deployTestRevertReason();
                await sendEvmMine();
                await contract.waitForDeployment();

                const signedTx = await prepareSignedTx({
                    contract,
                    account: ALICE,
                    methodName: "revertWithKnownError",
                    methodParameters: [],
                });
                const txHash = await sendRawTransaction(signedTx);
                await sendEvmMine();
                const txReceipt = await ETHERJS.getTransactionReceipt(txHash);

                const trace = await sendAndGetFullResponse("debug_traceTransaction", [
                    txReceipt?.hash,
                    { tracer: "4byteTracer" },
                ]);

                expect(trace.data.result["0x2b3d7bd2-0"]).to.eq(1);
            });

            it("prestateTracer", async () => {
                const contract = await deployTestContractBalances();
                await sendEvmMine();
                await contract.waitForDeployment();

                const contractOps = contract.connect(ALICE.signer());
                await contractOps.add(ALICE.address, 10);
                await contractOps.add(ALICE.address, 10);
                await contractOps.add(ALICE.address, 10);
                const txResponse = await contractOps.add(ALICE.address, 10);
                const txHash = txResponse.hash;
                await contractOps.add(ALICE.address, 10);
                await contractOps.add(ALICE.address, 10);
                await contractOps.add(ALICE.address, 10);

                await sendEvmMine();
                const txReceipt = await ETHERJS.getTransactionReceipt(txHash);

                const trace = await sendAndGetFullResponse("debug_traceTransaction", [
                    txReceipt?.hash,
                    { tracer: "prestateTracer", tracerConfig: { diffMode: false } },
                ]);

                const resultObj = trace.data.result[(await contract.getAddress()).toLowerCase()];
                expect(resultObj.balance).to.equal("0x0");
                expect(resultObj.nonce).to.equal(1);
                expect(resultObj.storage).to.deep.equal({
                    "0x723077b8a1b173adc35e5f0e7e3662fd1208212cb629f9c128551ea7168da722":
                        "0x000000000000000000000000000000000000000000000000000000000000001e",
                });
            });

            it("noopTracer", async () => {
                const contract = await deployTestRevertReason();
                await sendEvmMine();
                await contract.waitForDeployment();

                const signedTx = await prepareSignedTx({
                    contract,
                    account: ALICE,
                    methodName: "revertWithKnownError",
                    methodParameters: [],
                });
                const txHash = await sendRawTransaction(signedTx);
                await sendEvmMine();
                const txReceipt = await ETHERJS.getTransactionReceipt(txHash);

                const trace = await sendAndGetFullResponse("debug_traceTransaction", [
                    txReceipt?.hash,
                    { tracer: "noopTracer" },
                ]);

                expect(trace.data.result).to.be.empty;
            });

            it("muxTracer", async () => {
                const contract = await deployTestRevertReason();
                await sendEvmMine();
                await contract.waitForDeployment();

                const signedTx = await prepareSignedTx({
                    contract,
                    account: ALICE,
                    methodName: "revertWithKnownError",
                    methodParameters: [],
                });
                const txHash = await sendRawTransaction(signedTx);
                await sendEvmMine();
                const txReceipt = await ETHERJS.getTransactionReceipt(txHash);

                const trace = await sendAndGetFullResponse("debug_traceTransaction", [
                    txReceipt?.hash,
                    {
                        tracer: "muxTracer",
                        tracerConfig: { noopTracer: null, "4byteTracer": null },
                    },
                ]);

                expect(trace.data.result["4byteTracer"]["0x2b3d7bd2-0"]).to.eq(1);
                expect(trace.data.result.noopTracer).to.be.empty;
            });

            it("flatCallTracer", async () => {
                const contract = await deployTestRevertReason();
                await sendEvmMine();
                await contract.waitForDeployment();

                const signedTx = await prepareSignedTx({
                    contract,
                    account: ALICE,
                    methodName: "revertWithKnownError",
                    methodParameters: [],
                });
                const txHash = await sendRawTransaction(signedTx);
                await sendEvmMine();
                const txReceipt = await ETHERJS.getTransactionReceipt(txHash);

                const trace = await sendAndGetFullResponse("debug_traceTransaction", [
                    txReceipt?.hash,
                    { tracer: "flatCallTracer" },
                ]);
                const result = trace.data.result[0];
                expect(result.action.from).to.eq("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266");
                expect(result.action.callType).to.eq("call");
                expect(result.action.gas).to.eq("0x52fc");
                expect(result.action.input).to.eq("0x2b3d7bd2");
                expect(result.action.to).to.eq((await contract.getAddress()).toLowerCase());
                expect(result.action.value).to.eq("0x0");
                expect(result.error).to.eq("Reverted");
                expect(result.result.gasUsed).to.eq("0xb4");
                expect(result.result.output).to.eq("0x22aa4404");
                expect(result.subtraces).to.eq(0);
                expect(result.traceAddress).to.deep.eq([]);
                expect(result.transactionHash).to.eq(txReceipt?.hash);
                expect(result.transactionPosition).to.eq(0);
                expect(result.type).to.eq("call");
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
                await sendEvmMine();

                await new Promise((resolve) => setTimeout(resolve, 2000));

                // Get pending block timestamp
                const pendingTimestamp = await contract.getCurrentTimestamp();
                expect(pendingTimestamp).to.be.gt(0);

                // Wait 2 seconds
                await new Promise((resolve) => setTimeout(resolve, 2000));

                // Send first transaction
                const tx1 = await contract.recordTimestamp();

                // Wait 2 seconds
                await new Promise((resolve) => setTimeout(resolve, 2000));

                // Send second transaction
                const tx2 = await contract.recordTimestamp();

                // Wait 2 seconds
                await new Promise((resolve) => setTimeout(resolve, 2000));

                // Mine block to include both transactions
                await sendEvmMine();

                const [receipt1, receipt2] = await Promise.all([tx1.wait(), tx2.wait()]);

                // Get timestamps from contract events
                const event1 = receipt1.logs[0];
                const event2 = receipt2.logs[0];
                const recordedTimestamp1 = contract.interface.parseLog({
                    topics: event1.topics,
                    data: event1.data,
                })?.args.timestamp;
                const recordedTimestamp2 = contract.interface.parseLog({
                    topics: event2.topics,
                    data: event2.data,
                })?.args.timestamp;

                // Get the block timestamp (both transactions are in the same block)
                const block = await ETHERJS.getBlock(receipt1.blockNumber);
                const blockTimestamp = block!.timestamp;

                // Get all records from contract
                const records = await contract.getRecords();
                expect(records.length).to.equal(2);

                // Verify that the pending block timestamp matches the final block timestamp and transaction timestamps
                expect(pendingTimestamp).to.equal(blockTimestamp);
                expect(pendingTimestamp).to.equal(recordedTimestamp1);
                expect(pendingTimestamp).to.equal(recordedTimestamp2);

                // Verify that the stored records in the contract match the event data
                expect(records[0].timestamp).to.equal(recordedTimestamp1);
                expect(records[0].blockNumber).to.equal(receipt1.blockNumber);
                expect(records[1].timestamp).to.equal(recordedTimestamp2);
                expect(records[1].blockNumber).to.equal(receipt2.blockNumber);

                // Verify that both transactions see the same block timestamp
                expect(recordedTimestamp1).to.equal(blockTimestamp);
                expect(recordedTimestamp2).to.equal(blockTimestamp);

                // Verify that time is advancing
                const finalTimestamp = await contract.getCurrentTimestamp();
                expect(finalTimestamp).to.be.gt(pendingTimestamp);
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
