import { expect } from "chai";
import { network } from "hardhat";
import { Transaction } from "web3-types";

import { TestContractBalances } from "../../typechain-types";
import { CHARLIE } from "../helpers/account";
import {
    CHAIN_ID,
    HEX_PATTERN,
    calculateSlotPosition,
    deployTestContractBalances,
    send,
    sendExpect,
    sendGetBlockNumber,
    sendGetNonce,
    sendReset,
    toHex,
    toPaddedHex,
} from "../helpers/rpc";

// Test contract topics
const CONTRACT_TOPIC_ADD = "0x2728c9d3205d667bbc0eefdfeda366261b4d021949630c047f3e5834b30611ab";
const CONTRACT_TOPIC_SUB = "0xf9c652bcdb0eed6299c6a878897eb3af110dbb265833e7af75ad3d2c2f4a980c";

describe("Transaction: serial TestContractBalances", () => {
    let _contract: TestContractBalances;
    let _block: number;

    it("Resets blockchain", async () => {
        // HACK: sleeps for 50ms to avoid having the previous test interfering
        await new Promise((resolve) => setTimeout(resolve, 50));
        await sendReset();
        const blockNumber = await send("eth_blockNumber", []);
        expect(blockNumber).to.be.oneOf(["0x0", "0x1"]);
    });

    // it("Contract is deployed", async () => {
    //     _contract = await deployTestContractBalances();
    // });

    // it("Eth_getCode is not null for deployed contract", async () => {
    //     const deployedCode = await send("eth_getCode", [_contract.target, "latest"]);

    //     expect(deployedCode).not.eq("0x");
    // });

    // it("Deployment transaction receipt", async () => {
    //     const deploymentTransactionHash = _contract.deploymentTransaction()?.hash;
    //     expect(deploymentTransactionHash).not.eq(undefined);

    //     const receipt = await send("eth_getTransactionReceipt", [deploymentTransactionHash]);

    //     // Fields existence and null checks
    //     expect(receipt.contractAddress).not.eq(null, "receipt.contractAddress");
    //     expect(receipt.transactionHash).not.eq(null, "receipt.transactionHash");
    //     expect(receipt.transactionIndex).not.eq(null, "receipt.transactionIndex");
    //     expect(receipt.blockHash).not.eq(null, "receipt.blockHash");
    //     expect(receipt.blockNumber).not.eq(null, "receipt.blockNumber");
    //     expect(receipt.from).not.eq(null, "receipt.from");
    //     expect(receipt.to).eq(null, "receipt.to");
    //     expect(receipt.cumulativeGasUsed).not.eq(null, "receipt.cumulativeGasUsed");
    //     expect(receipt.gasUsed).not.eq(null, "receipt.gasUsed");
    //     expect(receipt.logs).not.eq(null, "receipt.logs");
    //     expect(receipt.logsBloom).not.eq(null, "receipt.logsBloom");
    //     expect(receipt.status).not.eq(null, "receipt.status");
    //     expect(receipt.effectiveGasPrice).not.eq(null, "receipt.effectiveGasPrice");

    //     // contract address
    //     const expectedContractAddress = _contract.target as string;
    //     const actualContractAddress = receipt.contractAddress as string;
    //     expect(expectedContractAddress.toLowerCase()).eq(
    //         actualContractAddress.toLowerCase(),
    //         "receipt.contractAddress",
    //     );

    //     // transaction hash
    //     const expectedTransactionHash = deploymentTransactionHash as string;
    //     const actualTransactionHash = receipt.transactionHash as string;
    //     expect(expectedTransactionHash.toLowerCase()).eq(
    //         actualTransactionHash.toLowerCase(),
    //         "receipt.transactionHash",
    //     );

    //     // transaction index
    //     const expectedTransactionIndex = "0x0";
    //     const actualTransactionIndex = receipt.transactionIndex as number;
    //     expect(expectedTransactionIndex).eq(actualTransactionIndex, "receipt.transactionIndex");

    //     // from
    //     expect(receipt.from.toLowerCase()).eq(CHARLIE.address.toLowerCase(), "receipt.from");

    //     // status
    //     const STATUS_SUCCESS = "0x1";
    //     expect(receipt.status).eq(STATUS_SUCCESS, "receipt.status");
    // });

    // it("Deployment transaction by hash", async () => {
    //     const deploymentTransactionHash = _contract.deploymentTransaction()?.hash;
    //     expect(deploymentTransactionHash).not.eq(undefined);

    //     const transaction: Transaction = await send("eth_getTransactionByHash", [deploymentTransactionHash]);

    //     expect(transaction.from).eq(CHARLIE.address, "tx.from");
    //     expect(transaction.to).eq(null, "tx.to");
    //     expect(transaction.value).eq("0x0", "tx.value");
    //     expect(transaction.gas).match(HEX_PATTERN, "tx.gas format");
    //     expect(transaction.gasPrice).match(HEX_PATTERN, "tx.gasPrice format");
    //     expect(transaction.input).match(HEX_PATTERN, "tx.input format");
    //     expect(transaction.nonce).eq("0x0", "tx.nonce");

    //     if (network.name === "stratus") {
    //         expect(transaction.chainId).eq(CHAIN_ID, "tx.chainId");
    //     }

    //     expect(transaction.v).match(HEX_PATTERN, "tx.v format");
    //     expect(transaction.r).match(HEX_PATTERN, "tx.r format");
    //     expect(transaction.s).match(HEX_PATTERN, "tx.s format");
    // });

    // it("Eth_call works on read function", async () => {
    //     // prepare transaction object
    //     const data = _contract.interface.encodeFunctionData("get", [CHARLIE.address]);
    //     const to = _contract.target;
    //     const from = CHARLIE.address;
    //     const transaction = { from: from, to: to, data: data };

    //     const currentCharlieBalance = await send("eth_call", [transaction, "latest"]);
    //     const expectedCharlieBalance = toPaddedHex(0, 32);

    //     expect(currentCharlieBalance).eq(expectedCharlieBalance);
    // });

    // it("Eth_call works on read function without from field", async () => {
    //     // prepare transaction object
    //     const data = _contract.interface.encodeFunctionData("get", [CHARLIE.address]);
    //     const to = _contract.target;
    //     const transaction = { to: to, data: data };

    //     const currentCharlieBalance = await send("eth_call", [transaction, "latest"]);
    //     const expectedCharlieBalance = toPaddedHex(0, 32);

    //     expect(currentCharlieBalance).eq(expectedCharlieBalance);
    // });

    // it("Performs add and sub", async () => {
    //     _block = await sendGetBlockNumber();

    //     // initial balance
    //     expect(await _contract.get(CHARLIE.address)).eq(0);

    //     // mutations
    //     let nonce = await sendGetNonce(CHARLIE.address);
    //     const contractOps = _contract.connect(CHARLIE.signer());
    //     await contractOps.add(CHARLIE.address, 10, { nonce: nonce++ });
    //     await contractOps.add(CHARLIE.address, 15, { nonce: nonce++ });
    //     await contractOps.sub(CHARLIE.address, 5, { nonce: nonce++ });

    //     // final balance
    //     expect(await _contract.get(CHARLIE.address)).eq(20);
    // });

    // it("Generates logs", async () => {
    //     let filter = { address: _contract.target, fromBlock: toHex(_block + 0) };

    //     // filter fromBlock
    //     (await sendExpect("eth_getLogs", [{ ...filter, fromBlock: toHex(_block + 0) }])).length(3);
    //     (await sendExpect("eth_getLogs", [{ ...filter, fromBlock: toHex(_block + 1) }])).length(3);
    //     (await sendExpect("eth_getLogs", [{ ...filter, fromBlock: toHex(_block + 2) }])).length(2);
    //     (await sendExpect("eth_getLogs", [{ ...filter, fromBlock: toHex(_block + 3) }])).length(1);
    //     (await sendExpect("eth_getLogs", [{ ...filter, fromBlock: toHex(_block + 4) }])).length(0);

    //     // filter topics
    //     (await sendExpect("eth_getLogs", [{ ...filter, topics: [CONTRACT_TOPIC_ADD] }])).length(2);
    //     (await sendExpect("eth_getLogs", [{ ...filter, topics: [CONTRACT_TOPIC_SUB] }])).length(1);
    // });

    // it("Balance matches storage", async () => {
    //     // calculate Charlie's position in balances mapping
    //     const balancesSlot = 0;
    //     const charlieStoragePosition = calculateSlotPosition(CHARLIE.address, balancesSlot);

    //     const expectedStorageValue = toPaddedHex(await _contract.get(CHARLIE.address), 32);
    //     const actualStorageValue = await send("eth_getStorageAt", [_contract.target, charlieStoragePosition, "latest"]);

    //     expect(actualStorageValue).eq(expectedStorageValue);
    // });
});
