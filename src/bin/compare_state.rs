use anyhow::Context;
use clap::Parser;
use rocksdb::Options;
use rocksdb::DB;
use stratus::eth::primitives::Address;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::SlotIndex;
use stratus::eth::primitives::SlotValue;
use stratus::eth::storage::rocks::rocks_cf::RocksCfRef;

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

    let db1 = DB::open_for_read_only(&Options::default(), &args.db1_path, false)?;
    let db2 = DB::open_for_read_only(&Options::default(), &args.db2_path, false)?;

    let cf1: RocksCfRef<(Address, SlotIndex, BlockNumber), SlotValue> = RocksCfRef::new(db1.into(), "account_slots_history")?;
    let cf2: RocksCfRef<(Address, SlotIndex, BlockNumber), SlotValue> = RocksCfRef::new(db2.into(), "account_slots_history")?;

    let mut differences = 0;

    let mut iter1 = cf1.iter_start();
    let mut iter2 = cf2.iter_start();

    while let (Some(result1), Some(result2)) = (iter1.next(), iter2.next()) {
        let ((address1, slot_index1, block_number1), value1) = result1.context("Error iterating over db1")?;
        let ((address2, slot_index2, block_number2), value2) = result2.context("Error iterating over db2")?;

        if block_number1 > args.max_block && block_number2 > args.max_block {
            continue;
        }

        // If block_numberA > max_block but block_numberB < max_block then cfB does not have the entry for that (and presumably future) blocks
        // Therefore cfB is already in another slot nad cfA need to iterate from that slot.
        if block_number1 > args.max_block {
            iter1 = cf1.iter_from((address2, slot_index2, block_number2), rocksdb::Direction::Forward)?;
            continue;
        }

        if block_number2 > args.max_block {
            iter2 = cf2.iter_from((address1, slot_index1, block_number1), rocksdb::Direction::Forward)?;
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
            differences += 1;
        }
    }

    // Check for any remaining entries in either database
    for result in iter1 {
        let ((address, slot_index, block_number), _) = result.context("Error iterating over remaining db1 entries")?;
        if block_number <= args.max_block {
            println!("Extra entry in db1: address: {:?}, slot: {:?}, block: {:?}", address, slot_index, block_number);
            differences += 1;
        }
    }

    for result in iter2 {
        let ((address, slot_index, block_number), _) = result.context("Error iterating over remaining db2 entries")?;
        if block_number <= args.max_block {
            println!("Extra entry in db2: address: {:?}, slot: {:?}, block: {:?}", address, slot_index, block_number);
            differences += 1;
        }
    }

    println!("Total differences found: {}", differences);

    Ok(())
}
