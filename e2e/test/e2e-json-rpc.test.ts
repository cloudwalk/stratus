import { expect } from "chai";
import { Block } from "web3-types";

import { ALICE, BOB } from "./helpers/account";
import { isStratus } from "./helpers/network";
import { CHAIN_ID, CHAIN_ID_DEC, TEST_BALANCE, ZERO, send, sendExpect, sendRawTransaction } from "./helpers/rpc";

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
    });

    describe("Account", () => {
        it("eth_getTransactionCount", async () => {
            (await sendExpect("eth_getTransactionCount", [ALICE])).eq(ZERO);
            (await sendExpect("eth_getTransactionCount", [ALICE, "latest"])).eq(ZERO);
        });
        it("eth_getBalance", async () => {
            (await sendExpect("eth_getBalance", [ALICE])).eq(TEST_BALANCE);
            (await sendExpect("eth_getBalance", [ALICE, "latest"])).eq(TEST_BALANCE);
        });
    });

    describe("Block", () => {
        it("eth_blockNumber", async function () {
            (await sendExpect("eth_blockNumber")).eq(ZERO);
        });
        it("eth_getBlockByNumber", async function () {
            let block: Block = await send("eth_getBlockByNumber", [ZERO, true]);
            expect(block.transactions.length).eq(0);
        });
        it("eth_getUncleByBlockHashAndIndex", async function () {
            if (isStratus) {
                (await sendExpect("eth_getUncleByBlockHashAndIndex", [ZERO, ZERO])).eq(null);
            }
        });
    });

    describe("Evm", () => {
        async function latest(): Promise<number> {
            return parseInt((await send("eth_getBlockByNumber", ["latest", false])).timestamp, 16);
        }
        it("evm_setNextBlockTimestamp", async () => {
            let target = 1;
            await send("evm_setNextBlockTimestamp", [`0x${target.toString(16)}`]);
            let txSigned = await ALICE.signWeiTransfer(BOB.address, 0);
            await sendRawTransaction(txSigned);
            expect(await latest()).eq(1);
            txSigned = await ALICE.signWeiTransfer(BOB.address, 0, 1);
            await sendRawTransaction(txSigned);
            expect(await latest()).not.eq(1);
        })
    });
});
