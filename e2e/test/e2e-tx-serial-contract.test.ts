import { expect } from "chai";

import { TestContractBalances } from "../typechain-types";
import { CHARLIE } from "./helpers/account";
import {
    calculateSlotPosition,
    deployTestContractBalances,
    send,
    sendExpect,
    sendGetBlockNumber,
    sendGetNonce,
    sendReset,
    toHex,
    toPaddedHex,
} from "./helpers/rpc";

// Test contract topics
const CONTRACT_TOPIC_ADD = "0x2728c9d3205d667bbc0eefdfeda366261b4d021949630c047f3e5834b30611ab";
const CONTRACT_TOPIC_SUB = "0xf9c652bcdb0eed6299c6a878897eb3af110dbb265833e7af75ad3d2c2f4a980c";

describe("Transaction: serial TestContractBalances", () => {
    var _contract: TestContractBalances;
    var _block: number;

    it("Resets blockchain", async () => {
        await sendReset();
    });
    it("Contract is deployed", async () => {
        _contract = await deployTestContractBalances();
    });

    it("Performs add and sub", async () => {
        _block = await sendGetBlockNumber();

        // initial balance
        expect(await _contract.get(CHARLIE.address)).eq(0);

        // mutations
        let nonce = await sendGetNonce(CHARLIE.address);
        const contractOps = _contract.connect(CHARLIE.signer());
        await contractOps.add(CHARLIE.address, 10, { nonce: nonce++ });
        await contractOps.add(CHARLIE.address, 15, { nonce: nonce++ });
        await contractOps.sub(CHARLIE.address, 5, { nonce: nonce++ });

        // final balance
        expect(await _contract.get(CHARLIE.address)).eq(20);
    });

    it("Generates logs", async () => {
        let filter = { address: _contract.target, fromBlock: toHex(_block + 0) };

        // filter fromBlock
        (await sendExpect("eth_getLogs", [{ ...filter, fromBlock: toHex(_block + 0) }])).length(3);
        (await sendExpect("eth_getLogs", [{ ...filter, fromBlock: toHex(_block + 1) }])).length(3);
        (await sendExpect("eth_getLogs", [{ ...filter, fromBlock: toHex(_block + 2) }])).length(2);
        (await sendExpect("eth_getLogs", [{ ...filter, fromBlock: toHex(_block + 3) }])).length(1);
        (await sendExpect("eth_getLogs", [{ ...filter, fromBlock: toHex(_block + 4) }])).length(1);

        // filter topics
        (await sendExpect("eth_getLogs", [{ ...filter, topics: [CONTRACT_TOPIC_ADD] }])).length(2);
        (await sendExpect("eth_getLogs", [{ ...filter, topics: [CONTRACT_TOPIC_SUB] }])).length(1);
    });

    it("Balance matches storage", async () => {
        // calculate Charlie's position in balances mapping
        const balancesSlot = 0;
        const charlieStoragePosition = calculateSlotPosition(CHARLIE.address, balancesSlot);

        const expectedStorageValue = toPaddedHex(await _contract.get(CHARLIE.address), 32);
        const actualStorageValue = await send("eth_getStorageAt", [_contract.target, charlieStoragePosition, "latest"]);

        expect(actualStorageValue).eq(expectedStorageValue);
    });
});
