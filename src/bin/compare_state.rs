use anyhow::Context;
use clap::Parser;
use ethereum_types::H160;
use ethereum_types::U256;
use ethereum_types::U64;
use rayon::prelude::*;
use rocksdb::properties::ESTIMATE_NUM_KEYS;
use rocksdb::Options;
use rocksdb::DB;
use stratus::eth::primitives::Address;
use stratus::eth::primitives::SlotIndex;
use stratus::eth::primitives::SlotValue;
use stratus::eth::storage::rocks::cf_versions::CfAccountSlotsHistoryValue;
use stratus::eth::storage::rocks::rocks_cf::RocksCfRef;
use stratus::eth::storage::rocks::types::AddressRocksdb;
use stratus::eth::storage::rocks::types::BlockNumberRocksdb;
use stratus::eth::storage::rocks::types::SlotIndexRocksdb;

static CHANGED_BLOCKS: [u64; 18] = [
    72648975, 72648976, 72649598, 72649599, 72656728, 72656729, 72662429, 72662430, 72667568, 72667569, 72671814, 72671815, 72679601, 72679602, 72680731,
    72680732, 72682152, 72682153,
];

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    new_db_path: String,

    #[arg(short, long)]
    old_db_path: String,

    #[arg(short, long)]
    max_block: u64,
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

    let differences = std::sync::atomic::AtomicUsize::new(0);

    let old_db_iter = cf2.iter_start();

    let progress_bar = indicatif::ProgressBar::new(num_keys);
    progress_bar.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} ({percentage}%) ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    println!("Starting state comparison");

    old_db_iter.par_bridge().try_for_each(|result| -> anyhow::Result<()> {
        let ((address, slot_index, block_number), value_old) = result.context("Error iterating over db2")?;

        if block_number.0.as_u64() > args.max_block {
            return Ok(());
        }

        let key = (
            Address::from(address.0).into(),
            SlotIndex::from(slot_index.0).into(),
            block_number.0.as_u64().into(),
        );

        match cf1.get(&key)? {
            Some(value_new) =>
                if value_new != SlotValue::from(value_old.0).into() {
                    println!("Difference found at address: {:?}, slot: {:?}, block: {:?}", address, slot_index, block_number);
                    println!("  Value old: {:?}", value_old);
                    println!("  Value new: {:?}", U256(value_new.into_inner().0));
                    differences.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                },
            None => {
                println!("Entry missing in new DB:");
                println!("  Old DB: address: {:?}, slot: {:?}, block: {:?}", address, slot_index, block_number);
                if CHANGED_BLOCKS.binary_search(&block_number.0.as_u64()).is_ok() {
                    let prev_block_key = (
                        Address::from(address.0).into(),
                        SlotIndex::from(slot_index.0).into(),
                        (block_number.0.as_u64() - 1).into(),
                    );
                    if let Ok(Some(prev_value)) = cf1.get(&prev_block_key) {
                        if prev_value == SlotValue::from(value_old.0).into() {
                            println!("Values are the same in the previous block.");
                        } else {
                            println!(
                                "Found in previous block. Old value: {:?}, Previous block value: {:?}",
                                value_old,
                                U256(prev_value.into_inner().0)
                            );
                        }
                    }
                }
                differences.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }

        progress_bar.inc(1);
        Ok(())
    })?;

    progress_bar.finish();
    println!("Total differences found: {}", differences.load(std::sync::atomic::Ordering::Relaxed));

    Ok(())
}
