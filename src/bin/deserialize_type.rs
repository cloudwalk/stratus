//! Helper that deserializes a type from STDIN to STDOUT.
//!
//! # Usage example:
//!
//! ```sh
//! cargo run --bin deserialize-type -- block < path/to/file
//! ```

#![allow(clippy::wildcard_imports)]

use std::fmt::Debug;
use std::io;
use std::io::Read;

use clap::Parser;
use const_hex::hex;
use serde::Deserialize;
use stratus::eth::storage::rocks::types::*;

#[derive(Parser)]
struct CliArgs {
    /// The type to desserialize to.
    type_: String,

    /// Is input in hexadecimal.
    #[arg(long)]
    hex: bool,
}

fn main() {
    let args = CliArgs::parse();

    let target_type = args.type_.trim().to_lowercase();
    let target_type = target_type.trim_end_matches("Rocksdb");

    let dispatch_map = &[
        ("Account", boxed_fn::<AccountRocksdb>()),
        ("Bytes", boxed_fn::<BytesRocksdb>()),
        ("Wei", boxed_fn::<WeiRocksdb>()),
        ("Nonce", boxed_fn::<NonceRocksdb>()),
        ("SlotValue", boxed_fn::<SlotValueRocksdb>()),
        ("Address", boxed_fn::<AddressRocksdb>()),
        ("BlockNumber", boxed_fn::<BlockNumberRocksdb>()),
        ("SlotIndex", boxed_fn::<SlotIndexRocksdb>()),
        ("Hash", boxed_fn::<HashRocksdb>()),
        ("Index", boxed_fn::<IndexRocksdb>()),
        ("Gas", boxed_fn::<GasRocksdb>()),
        ("MinerNonce", boxed_fn::<MinerNonceRocksdb>()),
        ("Difficulty", boxed_fn::<DifficultyRocksdb>()),
        ("BlockHeader", boxed_fn::<BlockHeaderRocksdb>()),
        ("UnixTime", boxed_fn::<UnixTimeRocksdb>()),
        ("LogsBloom", boxed_fn::<LogsBloomRocksdb>()),
        ("Size", boxed_fn::<SizeRocksdb>()),
        ("ChainId", boxed_fn::<ChainIdRocksdb>()),
        ("TransactionInput", boxed_fn::<TransactionInputRocksdb>()),
        ("Log", boxed_fn::<LogRocksdb>()),
        // ("Execution", boxed_fn::<ExecutionRocksdb>()),
        ("LogMined", boxed_fn::<LogMinedRockdb>()),
        ("TransactionMined", boxed_fn::<TransactionMinedRocksdb>()),
        ("Block", boxed_fn::<BlockRocksdb>()),
    ];

    let function = dispatch_map
        .iter()
        .find_map(|(typename, candidate)| (target_type == typename.to_lowercase()).then_some(candidate));

    let mut stdin = vec![];
    io::stdin().read_to_end(&mut stdin).expect("failed to read STDIN");

    println!("size = {}", stdin.len());

    if args.hex {
        stdin = hex::decode(stdin).expect("failed to decode HEX");
    }

    if let Some(function) = function {
        function(stdin);
    } else {
        eprintln!("didn't find a function for type '{}' in dispatch map", target_type);
        std::process::exit(127);
    }
}

fn boxed_fn<T>() -> Box<dyn Fn(Vec<u8>)>
where
    T: for<'de> Deserialize<'de> + Debug + 'static,
{
    let f = run::<T>;
    Box::new(f)
}

fn run<T>(input: Vec<u8>)
where
    T: for<'de> Deserialize<'de> + Debug,
{
    match bincode::deserialize::<T>(&input) {
        Ok(output) => println!("{output:#?}"),
        Err(err) => eprintln!("failed to deserialize to type '{}': {err:?}", std::any::type_name::<T>()),
    }
}
