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

    it("Eth_getCode is not null for deployed contract", async () => {
        const deployedCode = await send("eth_getCode", [_contract.target, "latest"]);

        expect(deployedCode).not.eq("0x");
    });

    it("Deployment transaction receipt fields", async () => {
        const deploymentTransactionHash = _contract.deploymentTransaction()?.hash;
        expect(deploymentTransactionHash).not.eq(undefined);
        
        const receipt = await send("eth_getTransactionReceipt", [deploymentTransactionHash]);
        expect(receipt.contractAddress).not.eq(null);
        expect(receipt.transactionHash).not.eq(null);
        expect(receipt.transactionIndex).not.eq(null);
        expect(receipt.blockHash).not.eq(null);
        expect(receipt.blockNumber).not.eq(null);
        expect(receipt.from).not.eq(null);
        expect(receipt.to).eq(null);
        expect(receipt.cumulativeGasUsed).not.eq(null);
        expect(receipt.gasUsed).not.eq(null);
        expect(receipt.logs).not.eq(null);
        expect(receipt.logsBloom).not.eq(null);
        expect(receipt.status).not.eq(null);
        expect(receipt.effectiveGasPrice).not.eq(null);

        const expectedContractAddress = _contract.target as string;
        const actualContractAddress = receipt.contractAddress as string;
        expect(expectedContractAddress.toLowerCase()).eq(actualContractAddress.toLowerCase());

        const expectedTransactionHash = deploymentTransactionHash as string;
        const actualTransactionHash = receipt.transactionHash as string;
        expect(expectedTransactionHash.toLowerCase()).eq(actualTransactionHash.toLowerCase());
    });

    it("Eth_call works on read function", async () => {
        // prepare transaction object
        const data = _contract.interface.encodeFunctionData("get", [CHARLIE.address]);
        const to = _contract.target;
        const from = CHARLIE.address;
        const transaction = { from: from, to: to, data: data };

        const currentCharlieBalance = await send("eth_call", [transaction, "latest"]);
        const expectedCharlieBalance = toPaddedHex(0, 32);

        expect(currentCharlieBalance).eq(expectedCharlieBalance);
    });

    it("Eth_call works on read function without from field", async () => {
        // prepare transaction object
        const data = _contract.interface.encodeFunctionData("get", [CHARLIE.address]);
        const to = _contract.target;
        const transaction = { to: to, data: data };

        const currentCharlieBalance = await send("eth_call", [transaction, "latest"]);
        const expectedCharlieBalance = toPaddedHex(0, 32);

        expect(currentCharlieBalance).eq(expectedCharlieBalance);
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
