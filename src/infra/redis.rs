
extern crate redis;

pub struct RedisStorage {
    pub client: redis::Client
}

impl RedisStorage {
    pub async fn new(url: &str) -> Self {
        tracing::info!("Redis connection pool created");

        let client = redis::Client::open(&url).unwrap();

        Self { client }
    }

    pub fn get_connection(&self) -> redis::Connection {
        tracing::info!("Redis get connection");
        self.client.get_connection().unwrap()
    }
}