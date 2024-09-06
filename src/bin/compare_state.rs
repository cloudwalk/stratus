use anyhow::Context;
use clap::Parser;
use ethereum_types::H160;
use ethereum_types::U256;
use ethereum_types::U64;
use rocksdb::properties::ESTIMATE_NUM_KEYS;
use rocksdb::Options;
use rocksdb::DB;
use stratus::eth::primitives::Address;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::SlotIndex;
use stratus::eth::primitives::SlotValue;
use stratus::eth::storage::rocks::cf_versions::CfAccountSlotsHistoryValue;
use stratus::eth::storage::rocks::rocks_cf::RocksCfRef;
use stratus::eth::storage::rocks::types::AddressRocksdb;
use stratus::eth::storage::rocks::types::BlockNumberRocksdb;
use stratus::eth::storage::rocks::types::SlotIndexRocksdb;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    new_db_path: String,

    #[arg(short, long)]
    old_db_path: String,

    #[arg(short, long)]
    max_block: BlockNumber,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Hash, Eq, PartialEq)]
struct AddressRocksdbOld(pub H160);
#[derive(serde::Serialize, serde::Deserialize, Debug, Hash, Eq, PartialEq)]
struct SlotIndexRocksdbOld(pub U256);
#[derive(serde::Serialize, serde::Deserialize, Debug, Hash, Eq, PartialEq)]
struct BlockNumberRocksdbOld(pub U64);
#[derive(serde::Serialize, serde::Deserialize, Debug, Hash, Eq, PartialEq, Clone)]
struct SlotValueRocksdbOld(pub U256);

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    println!("Opening databases");

    let db1 = DB::open_cf_for_read_only(&Options::default(), &args.new_db_path, ["account_slots_history"], false)?;
    println!("Opened db1");
    let db2 = DB::open_cf_for_read_only(&Options::default(), &args.old_db_path, ["account_slots_history"], false)?;
    println!("Opened db2");

    let cf1: RocksCfRef<(AddressRocksdb, SlotIndexRocksdb, BlockNumberRocksdb), CfAccountSlotsHistoryValue> =
        RocksCfRef::new(db1.into(), "account_slots_history")?;
    let cf2: RocksCfRef<(AddressRocksdbOld, SlotIndexRocksdbOld, BlockNumberRocksdbOld), SlotValueRocksdbOld> =
        RocksCfRef::new(db2.into(), "account_slots_history")?;
    let num_keys1 = cf1.property_int_value(ESTIMATE_NUM_KEYS).unwrap().unwrap_or(0);
    let num_keys2 = cf2.property_int_value(ESTIMATE_NUM_KEYS).unwrap().unwrap_or(0);
    let num_keys = std::cmp::max(num_keys1, num_keys2);

    let mut differences = 0;

    let old_db_iter = cf2.iter_start();

    let progress_bar = indicatif::ProgressBar::new(num_keys);
    let max_block: BlockNumberRocksdb = args.max_block.into();
    println!("Starting state comparison");
    for result in progress_bar.wrap_iter(old_db_iter) {
        let Ok(((address, slot_index, block_number), value_old)) = result.context("Error iterating over db2") else {
            continue;
        };

        if BlockNumberRocksdb::from(block_number.0.as_u64()) > max_block {
            continue;
        }

        let key = (
            Address::from(address.0).into(),
            SlotIndex::from(slot_index.0).into(),
            block_number.0.as_u64().into(),
        );

        match cf1.get(&key)? {
            Some(value_new) => {
                if value_new != SlotValue::from(value_old.0).into() {
                    println!(
                        "Difference found at address: {:?}, slot: {:?}, block: {:?}",
                        address, slot_index, block_number
                    );
                    println!("  Value old: {:?}", value_old);
                    println!("  Value new: {:?}", value_new);
                    differences += 1;
                }
            }
            None => {
                println!("Entry missing in new DB:");
                println!("  Old DB: address: {:?}, slot: {:?}, block: {:?}", address, slot_index, block_number);
                differences += 1;
            }
        }
    }

    progress_bar.finish();
    println!("Total differences found: {}", differences);

    Ok(())
}
