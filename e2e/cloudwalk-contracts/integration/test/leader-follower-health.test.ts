import { expect } from "chai";

import { sendAndGetFullResponse, sendWithRetry, updateProviderUrl } from "./helpers/rpc";

it("Test follower health is based on getting new blocks", async function () {
    updateProviderUrl("stratus-follower");
    var healthyResponse = await sendWithRetry("stratus_health", []);
    expect(healthyResponse).to.equal(true);
    // Kill port 3000 to interrupt leader
    require("child_process").execSync("killport 3000 -s sigterm", { stdio: "inherit" });

    await new Promise((resolve) => setTimeout(resolve, 5000));

    const unhealthyResponse = await sendAndGetFullResponse("stratus_health", []);
    expect(unhealthyResponse.data.error.code).to.equal(-32009);
    expect(unhealthyResponse.data.error.message).to.equal("Stratus is not ready to start servicing requests.");

    // Start the leader again
    require("child_process").execSync("just e2e-leader", { stdio: "inherit" });

    await new Promise((resolve) => setTimeout(resolve, 5000));

    var healthyResponse = await sendWithRetry("stratus_health", []);
    expect(healthyResponse).to.equal(true);
});
