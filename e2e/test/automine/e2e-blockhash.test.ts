import { expect } from "chai";

import { TestBlockHash } from "../../typechain-types";
import { CHARLIE } from "../helpers/account";
import { isStratus } from "../helpers/network";
import {
    ETHERJS,
    HASH_ZERO,
    SUCCESS,
    deployTestBlockHash,
    pollReceipt,
    send,
    sendClearCache,
    sendEvmMine,
    sendGetBlockNumber,
    toHex,
} from "../helpers/rpc";

// Reads the hash a block reports through the JSON-RPC interface.
async function blockHashOf(blockNumber: number): Promise<string> {
    const block = await send("eth_getBlockByNumber", [toHex(blockNumber), false]);
    expect(block, `block ${blockNumber} should exist`).to.not.be.null;
    return block.hash;
}

describe("BLOCKHASH opcode", () => {
    let contract: TestBlockHash;

    before(async () => {
        contract = await deployTestBlockHash();
    });

    it("yields zero for the block being executed", async () => {
        const [blockNumber, blockHash] = await contract.getCurrentBlockHash();

        expect(blockNumber).to.be.greaterThan(0n);
        expect(blockHash).eq(HASH_ZERO);
    });

    it("yields zero for blocks that were not mined yet", async () => {
        const latest = await sendGetBlockNumber();

        expect(await contract.getBlockHash(latest + 1)).eq(HASH_ZERO);
        expect(await contract.getBlockHash(latest + 1000)).eq(HASH_ZERO);
    });

    it("yields the hash the parent block reports", async () => {
        await sendEvmMine();

        const [parentNumber, parentHash] = await contract.getParentBlockHash();

        expect(parentHash).to.not.eq(HASH_ZERO);
        expect(parentHash).eq(await blockHashOf(Number(parentNumber)));
    });

    it("yields the hash of several consecutive blocks", async () => {
        await sendEvmMine();
        await sendEvmMine();

        // the parent of the executing block is the most recent one the opcode can reach
        const [parentNumber] = await contract.getParentBlockHash();

        for (let blockNumber = Number(parentNumber); blockNumber > Number(parentNumber) - 3; blockNumber--) {
            const blockHash = await contract.getBlockHash(blockNumber);
            expect(blockHash, `blockhash of block ${blockNumber}`).to.not.eq(HASH_ZERO);
            expect(blockHash, `blockhash of block ${blockNumber}`).eq(await blockHashOf(blockNumber));
        }
    });

    it("yields the parent hash of the block that mines the transaction", async () => {
        const receipt = await pollReceipt(contract.connect(CHARLIE.signer()).recordParentBlockHash());
        expect(receipt.status).eq(SUCCESS);

        const parentNumber = receipt.blockNumber - 1;
        const recorded = await contract.recordedBlockHashes(parentNumber);

        expect(recorded).to.not.eq(HASH_ZERO);
        expect(recorded).eq(await blockHashOf(parentNumber));

        // the opcode must agree with the parent hash stamped on the block that mined the transaction
        const minedBlock = await ETHERJS.getBlock(receipt.blockNumber);
        expect(recorded).eq(minedBlock?.parentHash);
    });

    it("yields the same hash for a call and for a transaction", async () => {
        const target = (await sendGetBlockNumber()) - 1;

        const receipt = await pollReceipt(contract.connect(CHARLIE.signer()).recordBlockHash(target));
        expect(receipt.status).eq(SUCCESS);

        const recorded = await contract.recordedBlockHashes(target);
        expect(recorded).to.not.eq(HASH_ZERO);
        expect(recorded).eq(await contract.getBlockHash(target));
    });

    it("yields blocks hashes read back from the permanent storage", async function () {
        if (!isStratus) {
            this.skip();
            return;
        }

        await sendEvmMine();

        // drops the hashes this node published while mining, forcing the reads to hit the permanent storage
        await sendClearCache();

        const [parentNumber, parentHash] = await contract.getParentBlockHash();

        expect(parentHash).to.not.eq(HASH_ZERO);
        expect(parentHash).eq(await blockHashOf(Number(parentNumber)));
        expect(await contract.getBlockHash(Number(parentNumber) - 1)).eq(await blockHashOf(Number(parentNumber) - 1));
    });

    it("chains every mined block to the hash of its parent", async () => {
        const latest = await sendGetBlockNumber();

        for (let blockNumber = latest; blockNumber > latest - 5; blockNumber--) {
            const block = await ETHERJS.getBlock(blockNumber);
            expect(block?.parentHash, `parent hash of block ${blockNumber}`).eq(await blockHashOf(blockNumber - 1));
        }
    });
});
