import { expect } from "chai";
import { TransactionReceipt, TransactionResponse, keccak256 } from "ethers";
import { Block, Bytes } from "web3-types";

import { TestContractBalances } from "../../typechain-types";
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
    prepareSignedTx,
    send,
    sendEvmMine,
    sendExpect,
    sendGetBlockNumber,
    sendGetNonce,
    sendRawTransaction,
    sendReset,
    toHex,
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
});
