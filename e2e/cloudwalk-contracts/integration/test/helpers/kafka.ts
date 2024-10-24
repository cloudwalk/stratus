import { Consumer, Kafka, KafkaMessage } from "kafkajs";

const kafka = new Kafka({
    clientId: "e2e",
    brokers: ["localhost:29092"],
});

let consumer: Consumer;

export async function initializeKafkaConsumer(groupId: string = "test-group"): Promise<void> {
    consumer = kafka.consumer({ groupId });
    await consumer.connect();
}

export async function subscribeToTopic(topic: string): Promise<void> {
    if (!consumer) {
        throw new Error("Kafka consumer not initialized. Call initializeKafkaConsumer first.");
    }
    await consumer.subscribe({ topic, fromBeginning: true });
}

export async function consumeMessages(timeoutMs: number = 5000): Promise<string[]> {
    if (!consumer) {
        throw new Error("Kafka consumer not initialized. Call initializeKafkaConsumer first.");
    }

    const messages: string[] = [];

    await consumer.run({
        eachMessage: async ({ message }: { message: KafkaMessage }) => {
            if (message.value) {
                messages.push(message.value.toString());
            }
        },
    });

    await new Promise((resolve) => setTimeout(resolve, timeoutMs));

    await consumer.stop();

    return messages;
}

export async function disconnectKafkaConsumer(): Promise<void> {
    if (consumer) {
        await consumer.disconnect();
    }
}
