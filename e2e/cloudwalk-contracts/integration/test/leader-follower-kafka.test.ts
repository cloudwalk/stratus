import { expect } from "chai";

import { consumeMessages, disconnectKafkaConsumer, initializeKafkaConsumer, subscribeToTopic } from "./helpers/kafka";
import { sendWithRetry, updateProviderUrl } from "./helpers/rpc";

describe("Leader & Follower Kafka integration test", function () {
    it("Validate initial Leader state and health", async function () {
        updateProviderUrl("stratus");
        const leaderNode = await sendWithRetry("stratus_state", []);
        expect(leaderNode.is_importer_shutdown).to.equal(true);
        expect(leaderNode.is_interval_miner_running).to.equal(true);
        expect(leaderNode.is_leader).to.equal(true);
        expect(leaderNode.miner_paused).to.equal(false);
        expect(leaderNode.transactions_enabled).to.equal(true);
        const leaderHealth = await sendWithRetry("stratus_health", []);
        expect(leaderHealth).to.equal(true);
    });

    it("Validate initial Follower state and health", async function () {
        updateProviderUrl("stratus-follower");
        const followerNode = await sendWithRetry("stratus_state", []);
        expect(followerNode.is_importer_shutdown).to.equal(false);
        expect(followerNode.is_interval_miner_running).to.equal(false);
        expect(followerNode.is_leader).to.equal(false);
        expect(followerNode.miner_paused).to.equal(false);
        expect(followerNode.transactions_enabled).to.equal(true);
        const followerHealth = await sendWithRetry("stratus_health", []);
        expect(followerHealth).to.equal(true);
    });

    describe("Leader & Follower Kafka integration test", function () {
        before(async function () {
            await initializeKafkaConsumer("stratus-consumer");
            await subscribeToTopic("stratus-events");
        });

        after(async function () {
            await disconnectKafkaConsumer();
        });

        it("Consume events from Kafka topic", async function () {
            const messages = await consumeMessages();
            expect(messages.length).to.be.greaterThan(0);

            console.log("Consumed messages:");
            messages.forEach((message, index) => {
                console.log(`Message ${index + 1}:`, JSON.stringify(message, null, 2));
            });
        });
    });
});
