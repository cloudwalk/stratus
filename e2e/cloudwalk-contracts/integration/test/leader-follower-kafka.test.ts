import { expect } from "chai";
import { ethers } from "hardhat";

import { consumeMessages, disconnectKafkaConsumer, initializeKafkaConsumer, subscribeToTopic } from "./helpers/kafka";
import { configureBRLC, deployBRLC, deployer, sendWithRetry, setDeployer, updateProviderUrl } from "./helpers/rpc";
import { GAS_LIMIT_OVERRIDE, brlcToken } from "./helpers/rpc";

describe("Leader & Follower Kafka integration test", function () {
    it("Validate initial Leader state and health", async function () {
        updateProviderUrl("stratus");
        const leaderHealth = await sendWithRetry("stratus_health", []);
        expect(leaderHealth).to.equal(true);
    });

    it("Validate initial Follower state and health", async function () {
        updateProviderUrl("stratus-follower");
        const followerHealth = await sendWithRetry("stratus_health", []);
        expect(followerHealth).to.equal(true);
    });

    describe("Deploy and configure BRLC contract", function () {
        updateProviderUrl("stratus");
        before(async function () {
            await setDeployer();
        });

        it("Validate deployer is main minter", async function () {
            await deployBRLC();
            await configureBRLC();

            expect(deployer.address).to.equal(await brlcToken.mainMinter());
            expect(await brlcToken.isMinter(deployer.address)).to.be.true;
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
