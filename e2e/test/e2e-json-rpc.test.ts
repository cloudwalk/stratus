import { expect } from "chai";
import { Block } from "web3-types";

import { ALICE, BOB } from "./helpers/account";
import { isStratus } from "./helpers/network";
import { CHAIN_ID, CHAIN_ID_DEC, TEST_BALANCE, ZERO, send, sendAndGetError, sendExpect, sendRawTransaction } from "./helpers/rpc";

describe("JSON-RPC", () => {
    describe("State", () => {
        it("debug_setHead / hardhat_reset", async () => {
            if (isStratus) {
                (await sendExpect("debug_setHead", [ZERO])).eq(ZERO);
            } else {
                (await sendExpect("hardhat_reset", [])).eq(true);
            }
        });

        it("Code for non-existent contract is 0x", async () => {
            const addressWithNothingDeployed = ALICE.address;
            (await sendExpect("eth_getCode", [addressWithNothingDeployed, "latest"])).eq("0x");
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
        async function latest(): Promise<{ timestamp: number; block_number: number }> {
            const block = await send("eth_getBlockByNumber", ["latest", false]);
            return { timestamp: parseInt(block.timestamp, 16), block_number: parseInt(block.number, 16) };
        }

        it("evm_mine", async () => {
            let prev_number = (await latest()).block_number;
            await send("evm_mine", []);
            expect((await latest()).block_number).eq(prev_number + 1);
        });

        describe("evm_setNextBlockTimestamp", () => {
            let target = Math.floor(Date.now() / 1000) + 10;
            it("Should set the next block timestamp", async () => {
                await send("evm_setNextBlockTimestamp", [target]);
                await send("evm_mine", []);
                expect((await latest()).timestamp).eq(target);
            })

            it("Should offset subsequent timestamps", async () => {
                await new Promise(resolve => setTimeout(resolve, 1000));
                await send("evm_mine", []);
                expect((await latest()).timestamp).to.be.greaterThan(target);
            })

            it("Should reset the changes when sending 0", async () => {
                await send("evm_setNextBlockTimestamp", [0]);
                let mined_timestamp = Math.floor(Date.now() / 1000);
                await send("evm_mine", []);
                let latest_timestamp = (await latest()).timestamp;
                expect(latest_timestamp).gte(mined_timestamp).lte(Math.floor(Date.now() / 1000));
            })

            it("Should handle negative offsets", async () => {
                const past = Math.floor(Date.now() / 1000);
                await new Promise(resolve => setTimeout(resolve, 2000));
                await send("evm_setNextBlockTimestamp", [past]);
                await send("evm_mine", []);
                expect((await latest()).timestamp).eq(past);
                await new Promise(resolve => setTimeout(resolve, 1000))
                await send("evm_mine", []);
                expect((await latest()).timestamp).to.be.greaterThan(past).lessThan(Math.floor(Date.now() / 1000));

                await send("evm_setNextBlockTimestamp", [0]);
            })
        });
    });

    describe("Subscription", () => {
        it("eth_subscribe over http fails with code 32603", async () => {
             const error = await sendAndGetError("eth_subscribe", ["newHeads"]);
             expect(error).to.not.be.null;
             expect(error.code).eq(-32603); // Internal error
        });
    });
});
