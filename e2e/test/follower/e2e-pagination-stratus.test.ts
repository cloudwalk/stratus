import { expect } from "chai";

import { ALICE, BOB } from "../helpers/account";
import { CHAIN_ID_DEC, send } from "../helpers/rpc";
import { FOLLOWER_URL, rpcCall, waitForFollowerBlock, waitForReceipt } from "./helpers";

const FAT_TX_DATA_BYTES = 50_000;

describe("Pagination (stratus-native importer format)", () => {
    it("syncs the follower through the stratus-native importer format", async () => {
        // fat contract deployment: the code always fails, but the fat data makes the response oversized
        const nonce = await send("eth_getTransactionCount", [ALICE.address]);
        const signedTx = await ALICE.signer().signTransaction({
            data: "0x" + "ab".repeat(FAT_TX_DATA_BYTES),
            chainId: CHAIN_ID_DEC,
            gasPrice: 0,
            gasLimit: 10_000_000,
            nonce: nonce,
        });
        const txHash = await send("eth_sendRawTransaction", [signedTx]);

        const receipt = await waitForReceipt(txHash);
        const fatBlockNumber = parseInt(receipt.blockNumber, 16);

        // the follower imports the fat block through the paginated stratus-format importer
        await waitForFollowerBlock(fatBlockNumber);

        // the follower serves the same block content; the leader block is requested thin because
        // its response limit rejects the full fat block over `eth_getBlockByNumber`
        const leaderBlock = await send("eth_getBlockByNumber", [receipt.blockNumber, false]);
        const followerBlock = await rpcCall(FOLLOWER_URL, "eth_getBlockByNumber", [receipt.blockNumber, true]);
        expect(followerBlock.result.hash).to.equal(leaderBlock.hash);
        expect(followerBlock.result.transactions).to.have.lengthOf(leaderBlock.transactions.length);
        expect(followerBlock.result.transactions[0].hash).to.equal(txHash);

        expect(followerBlock.result.gasUsed).to.equal(leaderBlock.gasUsed);
        expect(followerBlock.result.transactionsRoot).to.equal(leaderBlock.transactionsRoot);
        expect(followerBlock.result.logsBloom).to.equal(leaderBlock.logsBloom);
        expect(followerBlock.result.timestamp).to.equal(leaderBlock.timestamp);

        const followerReceipt = await rpcCall(FOLLOWER_URL, "eth_getTransactionReceipt", [txHash]);
        expect(followerReceipt.result).to.not.equal(null);
        expect(followerReceipt.result.blockNumber).to.equal(receipt.blockNumber);
        expect(followerReceipt.result.status).to.equal(receipt.status);
        expect(followerReceipt.result.gasUsed).to.equal(receipt.gasUsed);
        expect(followerReceipt.result.cumulativeGasUsed).to.equal(receipt.cumulativeGasUsed);
        expect(followerReceipt.result.effectiveGasPrice).to.equal(receipt.effectiveGasPrice);
        expect(followerReceipt.result.contractAddress).to.equal(receipt.contractAddress);
        expect(followerReceipt.result.logs).to.deep.equal(receipt.logs);
    });

    it("reproduces the leader's execution results for successful transactions", async () => {
        const nonce = await send("eth_getTransactionCount", [ALICE.address]);
        const signedTx = await ALICE.signer().signTransaction({
            to: BOB.address,
            value: 0,
            chainId: CHAIN_ID_DEC,
            gasPrice: 0,
            gasLimit: 21_000,
            nonce: nonce,
        });
        const txHash = await send("eth_sendRawTransaction", [signedTx]);

        const receipt = await waitForReceipt(txHash);
        const blockNumber = parseInt(receipt.blockNumber, 16);

        await waitForFollowerBlock(blockNumber);

        const leaderBlock = await send("eth_getBlockByNumber", [receipt.blockNumber, false]);
        const followerBlock = await rpcCall(FOLLOWER_URL, "eth_getBlockByNumber", [receipt.blockNumber, false]);
        expect(followerBlock.result.hash).to.equal(leaderBlock.hash);
        expect(followerBlock.result.gasUsed).to.equal(leaderBlock.gasUsed);
        expect(followerBlock.result.transactionsRoot).to.equal(leaderBlock.transactionsRoot);
        expect(followerBlock.result.logsBloom).to.equal(leaderBlock.logsBloom);

        const followerReceipt = await rpcCall(FOLLOWER_URL, "eth_getTransactionReceipt", [txHash]);
        expect(followerReceipt.result).to.not.equal(null);
        expect(followerReceipt.result.status).to.equal(receipt.status);
        expect(followerReceipt.result.gasUsed).to.equal(receipt.gasUsed);
        expect(followerReceipt.result.cumulativeGasUsed).to.equal(receipt.cumulativeGasUsed);
        expect(followerReceipt.result.logs).to.deep.equal(receipt.logs);

        const leaderBalance = await send("eth_getBalance", [BOB.address, receipt.blockNumber]);
        const followerBalance = await rpcCall(FOLLOWER_URL, "eth_getBalance", [BOB.address, receipt.blockNumber]);
        expect(followerBalance.result).to.equal(leaderBalance);
    });
});
