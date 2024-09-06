use anyhow::Context;
use clap::Parser;
use rocksdb::properties::ESTIMATE_NUM_KEYS;
use rocksdb::Options;
use rocksdb::DB;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::storage::rocks::rocks_cf::RocksCfRef;
use stratus::eth::storage::rocks::types::AddressRocksdb;
use stratus::eth::storage::rocks::types::BlockNumberRocksdb;
use stratus::eth::storage::rocks::types::SlotIndexRocksdb;
use stratus::eth::storage::rocks::types::SlotValueRocksdb;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    db1_path: String,

    #[arg(short, long)]
    db2_path: String,

    #[arg(short, long)]
    max_block: BlockNumber,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    println!("Opening databases");

    let db1 = DB::open_cf_for_read_only(&Options::default(), &args.db1_path, ["account_slots_history"], false)?;
    println!("Opened db1");
    let db2 = DB::open_cf_for_read_only(&Options::default(), &args.db2_path, ["account_slots_history"], false)?;
    println!("Opened db2");

    let cf1: RocksCfRef<(AddressRocksdb, SlotIndexRocksdb, BlockNumberRocksdb), SlotValueRocksdb> = RocksCfRef::new(db1.into(), "account_slots_history")?;
    let cf2: RocksCfRef<(AddressRocksdb, SlotIndexRocksdb, BlockNumberRocksdb), SlotValueRocksdb> = RocksCfRef::new(db2.into(), "account_slots_history")?;
    let num_keys1 = cf1.property_int_value(ESTIMATE_NUM_KEYS).unwrap().unwrap_or(0);
    let num_keys2 = cf2.property_int_value(ESTIMATE_NUM_KEYS).unwrap().unwrap_or(0);
    let num_keys = std::cmp::max(num_keys1, num_keys2);

    let mut differences = 0;

    let mut iter1 = cf1.iter_start();
    let mut iter2 = cf2.iter_start();

    let progress_bar = indicatif::ProgressBar::new(num_keys);
    let max_block: BlockNumberRocksdb = args.max_block.into();
    println!("Starting state comparison");
    while let (Some(result1), Some(result2)) = (iter1.next(), iter2.next()) {
        let Ok(((address1, slot_index1, block_number1), value1)) = result1.context("Error iterating over db1") else { continue };
        let Ok(((address2, slot_index2, block_number2), value2)) = result2.context("Error iterating over db2") else { continue };

        if block_number1 > max_block && block_number2 > max_block {
            progress_bar.inc(1);
            continue;
        }

        // If block_numberA > max_block but block_numberB < max_block then cfB does not have the entry for that (and presumably future) blocks
        // Therefore cfB is already in another slot and cfA need to iterate from that slot.
        if block_number1 > max_block {
            iter1 = cf1.iter_from((address2, slot_index2, block_number2), rocksdb::Direction::Forward)?;
            progress_bar.inc(1);
            continue;
        }

        if block_number2 > max_block {
            iter2 = cf2.iter_from((address1, slot_index1, block_number1), rocksdb::Direction::Forward)?;
            progress_bar.inc(1);
            continue;
        }

        if (address1, slot_index1, block_number1) != (address2, slot_index2, block_number2) {
            println!("Mismatched entries:");
            println!("  DB1: address: {:?}, slot: {:?}, block: {:?}", address1, slot_index1, block_number1);
            println!("  DB2: address: {:?}, slot: {:?}, block: {:?}", address2, slot_index2, block_number2);
            differences += 1;
        } else if value1 != value2 {
            println!(
                "Difference found at address: {:?}, slot: {:?}, block: {:?}",
                address1, slot_index1, block_number1
            );
            println!("  Value 1: {:?}", value1);
            println!("  Value 2: {:?}", value2);
            differences += 1;
        }

        progress_bar.inc(1);
    }

    // Check for any remaining entries in either database
    for result in iter1 {
        let ((address, slot_index, block_number), _) = result.context("Error iterating over remaining db1 entries")?;
        if block_number <= max_block {
            println!("Extra entry in db1: address: {:?}, slot: {:?}, block: {:?}", address, slot_index, block_number);
            differences += 1;
        }
        progress_bar.inc(1);
    }

    for result in iter2 {
        let ((address, slot_index, block_number), _) = result.context("Error iterating over remaining db2 entries")?;
        if block_number <= max_block {
            println!("Extra entry in db2: address: {:?}, slot: {:?}, block: {:?}", address, slot_index, block_number);
            differences += 1;
        }
        progress_bar.inc(1);
    }

    progress_bar.finish();
    println!("Total differences found: {}", differences);

    Ok(())
}
