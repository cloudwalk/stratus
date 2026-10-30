import axios from "axios";
import { expect } from "chai";
import { ChildProcess, spawn } from "child_process";
import { keccak256 } from "ethers";
import { closeSync, mkdtempSync, openSync, readFileSync, rmSync } from "fs";
import { createServer } from "net";
import { tmpdir } from "os";
import { join, resolve } from "path";

import { ALICE, BOB } from "../helpers/account";
import { isStratus } from "../helpers/network";

async function freePort(): Promise<number> {
    const server = createServer();
    await new Promise<void>((resolve, reject) => {
        server.once("error", reject);
        server.listen(0, "127.0.0.1", resolve);
    });
    const address = server.address();
    if (!address || typeof address === "string") throw new Error("expected a TCP address");
    await new Promise<void>((resolve, reject) => server.close((error) => (error ? reject(error) : resolve())));
    return address.port;
}

describe("Journal recovery", () => {
    it("preserves the pending header and transaction order across a leader restart", async function () {
        if (!isStratus) this.skip();
        this.timeout(120_000);

        const root = resolve(__dirname, "../../..");
        const binary = process.env.STRATUS_BINARY || join(root, "target/debug/stratus");
        const directory = mkdtempSync(join(tmpdir(), "stratus-journal-recovery-"));
        const prefix = join(directory, "node");
        const logPath = join(directory, "node.log");
        const port = await freePort();
        const url = `http://127.0.0.1:${port}`;
        let child: ChildProcess | undefined;
        let exit: Promise<void> | undefined;

        async function rpc(method: string, params: any[] = []): Promise<any> {
            const response = await axios.post(url, { jsonrpc: "2.0", id: 1, method, params }, { timeout: 2000 });
            if (response.data.error) throw new Error(JSON.stringify(response.data.error));
            return response.data.result;
        }

        async function start(): Promise<void> {
            const log = openSync(logPath, "a");
            try {
                child = spawn(
                    binary,
                    [
                        "--leader",
                        "--config",
                        join(root, "config/stratus.local.toml"),
                        "--address",
                        `127.0.0.1:${port}`,
                        "--metrics-exporter-address",
                        "127.0.0.1:0",
                        "--rocks-path-prefix",
                        prefix,
                        "--rocks-file-descriptors-limit",
                        "1024",
                        "--block-mode",
                        "external",
                    ],
                    { cwd: root, stdio: ["ignore", log, log] },
                );
            } finally {
                closeSync(log);
            }
            let spawnError: Error | undefined;
            exit = new Promise<void>((resolve) => {
                child!.once("error", (error) => {
                    spawnError = error;
                });
                child!.once("close", () => resolve());
            });
            const deadline = Date.now() + 45_000;
            while (Date.now() < deadline) {
                if (spawnError) throw spawnError;
                if (child.exitCode !== null || child.signalCode !== null) {
                    throw new Error(readFileSync(logPath, "utf8"));
                }
                try {
                    await rpc("eth_chainId");
                    return;
                } catch {
                    await new Promise((resolve) => setTimeout(resolve, 100));
                }
            }
            throw new Error(`leader startup timed out: ${readFileSync(logPath, "utf8")}`);
        }

        async function stop(signal: NodeJS.Signals): Promise<void> {
            child?.kill(signal);
            await exit;
            child = undefined;
        }

        try {
            await start();
            const latest = await rpc("eth_blockNumber");
            const nonce = Number(await rpc("eth_getTransactionCount", [ALICE.address, "pending"]));
            const hashes: string[] = [];
            let pendingNumber = "";
            for (let index = 0; index < 3; index++) {
                const signed = await ALICE.signWeiTransfer(BOB.address, index + 1, nonce + index);
                const hash = await rpc("eth_sendRawTransaction", [signed]);
                expect(hash).to.equal(keccak256(signed));
                pendingNumber = "0x" + (BigInt(latest) + 1n).toString(16);
                hashes.push(hash);
            }
            expect(await rpc("eth_blockNumber")).to.equal(latest);
            const journalPath = join(
                `${prefix}-rocksdb`,
                "transaction-journal",
                `${BigInt(pendingNumber).toString().padStart(20, "0")}.journal`,
            );
            const journal = readFileSync(journalPath, "utf8");
            const header = JSON.parse(journal.slice(0, journal.indexOf("}") + 1));
            expect(BigInt(header.number)).to.equal(BigInt(pendingNumber));

            await stop("SIGKILL");
            await new Promise((resolve) => setTimeout(resolve, 1100));
            await start();

            const recovered = await rpc("eth_getBlockByNumber", [pendingNumber, false]);
            expect(recovered).to.be.an("object");
            expect(BigInt(recovered.number)).to.equal(BigInt(header.number));
            expect(BigInt(recovered.timestamp)).to.equal(BigInt(header.timestamp));
            expect(recovered.transactions).to.deep.equal(hashes);
            for (const [index, hash] of hashes.entries()) {
                const receipt = await rpc("eth_getTransactionReceipt", [hash]);
                expect(receipt.blockHash).to.equal(recovered.hash);
                expect(Number(receipt.transactionIndex)).to.equal(index);
                expect(receipt.status).to.equal("0x1");
            }
            expect(Number(await rpc("eth_getTransactionCount", [ALICE.address, "latest"]))).to.equal(nonce + 3);
        } finally {
            await stop("SIGKILL");
            rmSync(directory, { recursive: true, force: true });
        }
    });
});
