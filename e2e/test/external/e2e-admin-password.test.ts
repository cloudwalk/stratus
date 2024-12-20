import { expect } from "chai";
import { send, sendAndGetError, sendReset } from "../helpers/rpc";

describe("Admin Password", () => {
    describe("With ADMIN_PASSWORD set", () => {
        before(async () => {
            process.env.ADMIN_PASSWORD = "test123";
            await sendReset();
        });

        after(() => {
            delete process.env.ADMIN_PASSWORD;
        });

        it("should reject requests without password", async () => {
            const error = await sendAndGetError("stratus_enableTransactions", []);
            expect(error.code).eq(-32603); // Internal error
            expect(error.message).to.contain("Invalid password");
        });

        it("should reject requests with wrong password", async () => {
            const headers = { Authorization: "Password wrong123" };
            const error = await sendAndGetError("stratus_enableTransactions", [], headers);
            expect(error.code).eq(-32603); // Internal error
            expect(error.message).to.contain("Invalid password");
        });

        it("should accept requests with correct password", async () => {
            const headers = { Authorization: "Password test123" };
            const result = await send("stratus_enableTransactions", [], headers);
            expect(result).to.be.true;

            // Cleanup - disable transactions
            await send("stratus_disableTransactions", [], headers);
        });
    });

    describe("Without ADMIN_PASSWORD set", () => {
        before(async () => {
            delete process.env.ADMIN_PASSWORD;
            await sendReset();
        });

        it("should accept requests without password", async () => {
            const result = await send("stratus_enableTransactions", []);
            expect(result).to.be.true;

            // Cleanup - disable transactions
            await send("stratus_disableTransactions", []);
        });

        it("should accept requests with any password", async () => {
            const headers = { Authorization: "Password random123" };
            const result = await send("stratus_enableTransactions", [], headers);
            expect(result).to.be.true;

            // Cleanup - disable transactions
            await send("stratus_disableTransactions", [], headers);
        });
    });
});
