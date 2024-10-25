import { expect } from "chai";
import { ethers } from "hardhat";

import { consumeMessages, disconnectKafkaConsumer, initializeKafkaConsumer, subscribeToTopic } from "./helpers/kafka";
import { configureBRLC, deployBRLC, deployer, sendWithRetry, setDeployer, updateProviderUrl } from "./helpers/rpc";
import { GAS_LIMIT_OVERRIDE, brlcToken } from "./helpers/rpc";

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

    describe("Deploy and configure BRLC contract using transaction forwarding from follower to leader", function () {
        before(async function () {
            await setDeployer();
        });

        it("Validate deployer is main minter", async function () {
            updateProviderUrl("stratus-follower");

            await deployBRLC();
            await configureBRLC();

            expect(deployer.address).to.equal(await brlcToken.mainMinter());
            expect(await brlcToken.isMinter(deployer.address)).to.be.true;

            updateProviderUrl("stratus");
        });
    });

    describe("Leader & Follower Kafka integration test", function () {
        before(async function () {
            await initializeKafkaConsumer("stratus-consumer");
            await subscribeToTopic("stratus-events");
        });

        after(async function () {
            await disconnectKafkaConsumer();
        });

        it("Makes a transaction and send events to Kafka", async function () {
            const wallet = ethers.Wallet.createRandom().connect(ethers.provider);

            await brlcToken.mint(wallet.address, 10, { gasLimit: GAS_LIMIT_OVERRIDE });

            const messages = await consumeMessages();
            expect(messages.length).to.be.greaterThan(0);

            messages.forEach((message, index) => {
                expect(JSON.parse(message).transfers[0].credit_party_address.toLowerCase()).to.be.eql(
                    wallet.address.toLowerCase(),
                );
                expect(JSON.parse(message).transfers[0].amount).to.equal("0.00001");
            });
        });
    });
});
