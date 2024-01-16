
extern crate redis;
use crate::eth::miner::BlockMiner;

#[derive(Debug,Clone)]
pub struct RedisStorage {
    pub client: redis::Client,
    pub url: String,
}

impl RedisStorage {
    pub async fn new(url: &str) -> eyre::Result<Self> {
        tracing::info!("Redis connection pool created");

        let client = redis::Client::open(url).unwrap();

        let redis_storage = Self {
            client,
            url: url.to_string(),
        };

        let genesis = BlockMiner::genesis();
        let block_number = genesis.header.number;
        let json = serde_json::to_string(&genesis.clone()).unwrap();
        let number: u64 = block_number.clone().into();
        let key = "BLOCK_".to_string() + &number.to_string();

        let mut con = redis_storage.get_connection();
        let _: () = redis::cmd("FLUSHALL").execute(&mut con);
        let _: () = redis::cmd("SET").arg(key).arg(json).execute(&mut con);
        let _: () = redis::cmd("SET").arg("CURRENT_BLOCK").arg(number).execute(&mut con);

        Ok(redis_storage)
    }

    pub fn get_connection(&self) -> redis::Connection {
        tracing::info!("Redis get connection");
        self.client.get_connection().unwrap()
    }
}