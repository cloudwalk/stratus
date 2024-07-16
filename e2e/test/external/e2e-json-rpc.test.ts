import { expect } from "chai";
import { TransactionResponse, keccak256 } from "ethers";

import { ALICE, BOB } from "../helpers/account";
import { BlockMode, currentBlockMode, isStratus } from "../helpers/network";
import { CHAIN_ID, CHAIN_ID_DEC, ETHERJS, HASH_ZERO, HEX_PATTERN, ONE, send, sendEvmMine, sendExpect, sendGetNonce, sendRawTransaction, sendReset, TEST_BALANCE, ZERO } from "../helpers/rpc";
import { Block, Bytes } from "web3-types";

describe("JSON-RPC", () => {
    before(() => {
        expect(currentBlockMode()).eq(BlockMode.External, "Wrong block mining mode is used");
    });

    describe("State", () => {
        it("debug_setHead", async () => {
            if (isStratus) {
                (await sendExpect("debug_setHead", [ZERO])).eq(ZERO);
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
                expect(client).eq("stratus")
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

    describe("Transaction", () => {
        describe("eth_getTransactionByHash", () => {
            it("returns transaction before and after minting", async () => {
                const amount = 1;
                const nonce = await sendGetNonce(ALICE);

                const signedTx = await ALICE.signWeiTransfer(BOB.address, amount, nonce);
                const txHash = keccak256(signedTx);
                await sendRawTransaction(signedTx);
                const txResponseAfterSending: TransactionResponse | null = await ETHERJS.getTransaction(txHash);

                await sendEvmMine();
                const txResponseAfterMinting: TransactionResponse | null = await ETHERJS.getTransaction(txHash);

                expect(txResponseAfterSending).exist;
                expect(txResponseAfterMinting).exist;
                expect(txResponseAfterSending?.hash).eq(txHash);
                expect(txResponseAfterMinting?.hash).eq(txHash);
            });
        });
    });
});
