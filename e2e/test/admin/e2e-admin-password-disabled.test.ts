import { expect } from "chai";
import { send, sendReset } from "../helpers/rpc";

describe("Admin Password (without password set)", () => {
    before(async () => {
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
