import { expect } from "chai";

import { send, sendAndGetError, sendReset } from "../helpers/rpc";

describe("Admin Password (with password set)", () => {
    before(async () => {
        await sendReset();
    });

    it("should reject requests without password", async () => {
        const error = await sendAndGetError("stratus_enableTransactions", []);
        expect(error.code).eq(7004); // Internal error
        expect(error.message).to.contain("Incorrect password");
    });

    it("should reject requests with wrong password", async () => {
        const headers = { Authorization: "Password wrong123" };
        const error = await sendAndGetError("stratus_enableTransactions", [], headers);
        expect(error.code).eq(7004); // Internal error
        expect(error.message).to.contain("Incorrect password");
    });

    it("should accept requests with correct password", async () => {
        const headers = { Authorization: "Password test123" };
        const result = await send("stratus_enableTransactions", [], headers);
        expect(result).to.be.true;

        // Cleanup - disable transactions
        await send("stratus_disableTransactions", [], headers);
    });
});
