import { expect } from "chai";
import { WebSocketProvider } from "ethers";

import { sendAndGetFullResponse, sendWithRetry, updateProviderUrl } from "./helpers/rpc";

test("Test follower health is based on getting new blocks", async function () {
    updateProviderUrl("stratus-follower");
    var healthyResponse = await sendWithRetry("stratus_health", []);
    expect(healthyResponse).to.equal(true);

    let ws = new WebSocketProvider("ws://localhost:3001");
    // check that ws is connected
    expect(await ws.getBlockNumber()).to.be.a("number");

    // Kill port 3000 to interrupt leader
    require("child_process").execSync("killport 3000 -s sigterm", { stdio: "inherit" });

    await new Promise((resolve) => setTimeout(resolve, 5000));

    const unhealthyResponse = await sendAndGetFullResponse("stratus_health", []);
    await new Promise((resolve) => setTimeout(resolve, 300));

    expect(unhealthyResponse.data.error.code).to.equal(7001);
    expect(unhealthyResponse.data.error.message).to.equal("Stratus is not ready to start servicing requests.");

    // check that the ws connection was closed
    expect(ws.websocket.readyState).to.not.equal(WebSocket.OPEN);

    // Start the leader again
    require("child_process").execSync("just e2e-leader", { stdio: "inherit" });

    await new Promise((resolve) => setTimeout(resolve, 5000));

    var healthyResponse = await sendWithRetry("stratus_health", []);

    expect(healthyResponse).to.equal(true);
});

test("stratus_disableRestartOnUnhealthy should not close connection", async function () {
    updateProviderUrl("stratus-follower");
    var healthyResponse = await sendWithRetry("stratus_health", []);
    await sendWithRetry("stratus_disableRestartOnUnhealthy", []);
    expect(healthyResponse).to.equal(true);

    let ws = new WebSocketProvider("ws://localhost:3001");
    // check that ws is connected
    expect(await ws.getBlockNumber()).to.be.a("number");

    // Kill port 3000 to interrupt leader
    require("child_process").execSync("killport 3000 -s sigterm", { stdio: "inherit" });

    await new Promise((resolve) => setTimeout(resolve, 5000));

    const unhealthyResponse = await sendAndGetFullResponse("stratus_health", []);
    await new Promise((resolve) => setTimeout(resolve, 300));

    expect(unhealthyResponse.data.error.code).to.equal(7001);
    expect(unhealthyResponse.data.error.message).to.equal("Stratus is not ready to start servicing requests.");

    // check that the ws connection was closed
    expect(ws.websocket.readyState).to.equal(WebSocket.OPEN);

    // Start the leader again
    require("child_process").execSync("just e2e-leader", { stdio: "inherit" });

    await new Promise((resolve) => setTimeout(resolve, 5000));

    var healthyResponse = await sendWithRetry("stratus_health", []);

    expect(healthyResponse).to.equal(true);
});
