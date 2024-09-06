use anyhow::Context;
use clap::Parser;
use rocksdb::{DB, Options};
use stratus::eth::storage::rocks::rocks_cf::RocksCfRef;
use stratus::eth::storage::rocks::rocks_state::RocksStorageState;
use stratus::eth::primitives::{Address, BlockNumber, SlotIndex, SlotValue};

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

    for result in cf1.iter_start() {
        let ((address, slot_index, block_number), value1) = result.context("Error iterating over db1")?;

        if block_number > args.max_block {
            continue;
        }

        if let Some(value2) = cf2.get(&(address, slot_index, block_number))? {
            if value1 != value2 {
                println!("Difference found at address: {:?}, slot: {:?}, block: {:?}", address, slot_index, block_number);
                differences += 1;
            }
        } else {
            println!("Entry missing in db2 for address: {:?}, slot: {:?}, block: {:?}", address, slot_index, block_number);
            differences += 1;
        }
    }

    println!("Total differences found: {}", differences);

    Ok(())
}
