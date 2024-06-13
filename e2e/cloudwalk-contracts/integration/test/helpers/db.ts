import { Client } from 'pg';

export async function getDbClient() {
    const client = new Client({
        host: 'localhost',
        port: 5432,
        user: 'postgres',
        password: '123',
        database: 'stratus'
    });

    await client.connect();

    return client;
}