
extern crate redis;

#[derive(Debug,Clone)]
pub struct RedisStorage {
    pub client: redis::Client,
    pub url: String,
}

impl RedisStorage {
    pub async fn new(url: &str) -> eyre::Result<Self> {
        tracing::info!("Redis connection pool created");

        let client = redis::Client::open(url.clone()).unwrap();

        Ok(Self {
            client,
            url: url.to_string(),
        })
    }

    pub fn get_connection(&self) -> redis::Connection {
        tracing::info!("Redis get connection");
        self.client.get_connection().unwrap()
    }
}