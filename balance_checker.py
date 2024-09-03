import argparse
import asyncio
import csv
from typing import Iterable, List, Optional, cast

from google.cloud import bigquery
from google.cloud.bigquery.table import RowIterator
from tqdm import tqdm
from web3 import AsyncHTTPProvider, AsyncWeb3
from web3.middleware import validation
from web3.types import BlockIdentifier

validation.METHODS_TO_VALIDATE = []
MERCHANT_WALLETS_SQL = """
WITH wallet_addresses as (SELECT
   CASE
   WHEN u.role IN ('cardholder','merchant','iso','referral') THEN 'merchant_balance'
   ELSE u.role END AS name,
   w.brlc_address AS address,
  FROM `infinitepay-production.maindb.wallets` w
  LEFT JOIN `infinitepay-production.maindb.cardholders` c ON c.user_id = w.user_id
  LEFT JOIN `infinitepay-production.maindb.users` u ON u.id = w.user_id
  WHERE w.user_id IS NOT NULL
  AND w.brlc_address IS NOT NULL
  GROUP BY name, w.brlc_address)
SELECT address
FROM wallet_addresses
WHERE name = 'merchant_balance'
"""


async def get_approximate_block_number_from_timestamp(
    w3: AsyncWeb3, target_timestamp: int
) -> int:
    latest_block = await w3.eth.get_block("latest")
    current_block_number: int | None = (
        latest_block["number"] if "number" in latest_block else None
    )
    current_block_timestamp: int | None = (
        latest_block["timestamp"] if "timestamp" in latest_block else None
    )

    if current_block_number is None or current_block_timestamp is None:
        raise ValueError("Required block information is missing")

    time_difference: int = current_block_timestamp - target_timestamp
    block_number_difference: int = time_difference
    target_block_number: int = current_block_number - block_number_difference
    target_block_number = max(0, target_block_number)

    return target_block_number


async def find_closest_block_by_timestamp(w3: AsyncWeb3, target_timestamp: int) -> int:
    approximate_block: int = await get_approximate_block_number_from_timestamp(
        w3, target_timestamp
    )
    block = await w3.eth.get_block(approximate_block)

    if block is None:
        raise ValueError("Failed to retrieve block information")

    block_timestamp: int | None = block.get("timestamp")

    if block_timestamp is None:
        raise ValueError("Block timestamp is missing")

    if block_timestamp == target_timestamp:
        return approximate_block

    current_block = approximate_block
    step = 1

    if block_timestamp < target_timestamp:
        while block_timestamp < target_timestamp:
            current_block += step
            block = await w3.eth.get_block(current_block)
            if block is None:
                raise ValueError(
                    f"Failed to retrieve block information for block {current_block}"
                )
            block_timestamp = block.get("timestamp")
            if block_timestamp is None:
                raise ValueError(
                    f"Block timestamp is missing for block {current_block}"
                )
            step += 1
        return max(current_block - 1, 0)
    else:
        while block_timestamp > target_timestamp:
            current_block -= step
            if current_block < 0:
                return 0
            block = await w3.eth.get_block(current_block)
            if block is None:
                raise ValueError(
                    f"Failed to retrieve block information for block {current_block}"
                )
            block_timestamp = block.get("timestamp")
            if block_timestamp is None:
                raise ValueError(
                    f"Block timestamp is missing for block {current_block}"
                )
            step -= 1
        return current_block


async def get_total_supply(
    w3: AsyncWeb3,
    contract_address: str,
    block_number: Optional[BlockIdentifier] = "latest",
) -> float:
    data = bytes.fromhex("18160ddd")
    try:
        result = await w3.eth.call({"to": contract_address, "data": data}, block_number)

        total_supply = int(result.hex(), 16) / 10**6
        return total_supply
    except Exception as e:
        print(f"Error getting total supply for contract {contract_address}: {str(e)}")
        raise e


async def get_single_balance(
    account: str, w3: AsyncWeb3, contract_address: str, block_number: int
) -> tuple[str, float]:
    formatted_account = account.lower().replace("0x", "")
    data = bytes.fromhex(f"70a08231000000000000000000000000{formatted_account}")

    try:
        result = await w3.eth.call({"to": contract_address, "data": data}, block_number)
        balance = int(result.hex(), 16) / 10**6
        return account, balance
    except Exception as e:
        print(f"Error getting balance for account {account}: {str(e)}")
        return account, 0


async def get_balances(
    w3: AsyncWeb3,
    contract_address: str,
    account_addresses: Iterable[str],
    block_number: int,
    n: int = 0,
) -> dict[str, float]:
    print(f"Getting {n} balances")
    balances = {}
    buffer_size = 5000
    semaphore = asyncio.Semaphore(buffer_size)
    progress_bar = tqdm(total=n, desc="Processing balances")

    async def process_balance(account):
        async with semaphore:
            account, balance = await get_single_balance(
                account, w3, contract_address, block_number
            )
            balances[account] = balance
            progress_bar.update(1)

    async with asyncio.TaskGroup() as tg:
        for account in tqdm(account_addresses, total=n, desc="Creating tasks"):
            tg.create_task(process_balance(account))

    progress_bar.close()
    return balances


def aggregate_balances(balances: dict[str, float]) -> float:
    print("Aggregating balances")
    return sum(balance for balance in balances.values() if balance is not None)


def get_merchants() -> RowIterator:
    print("Getting merchants")
    client = bigquery.Client(project="infinitepay-production")
    query_job = client.query(MERCHANT_WALLETS_SQL)
    results = query_job.result()

    return results


def address_generator(rows: RowIterator) -> Iterable[str]:
    for row in rows:
        yield row["address"]


def get_merchants_csv(file_path: str) -> List[str]:
    print(f"Reading merchant addresses from CSV file: {file_path}")
    addresses = []
    try:
        with open(file_path, "r") as csvfile:
            csv_reader = csv.reader(csvfile)
            for row in csv_reader:
                if row:  # Check if the row is not empty
                    bytes.fromhex(row[0].strip().lower().replace("0x", ""))
                    addresses.append(
                        row[0].strip()
                    )  # Assume the address is in the first (and only) column
    except FileNotFoundError:
        print(f"Error: CSV file not found at {file_path}")
    except csv.Error as e:
        print(f"Error reading CSV file: {e}")

    print(f"Found {len(addresses)} addresses in the CSV file")
    return addresses


async def async_main():
    parser = argparse.ArgumentParser(
        description="Check Ethereum balances at a specific block or date"
    )
    parser.add_argument("--chain_url", required=True, help="Ethereum node RPC URL")
    parser.add_argument(
        "--date", type=str, help="Date to check balances at (DD/MM/YYYY)"
    )
    parser.add_argument(
        "--block_number", type=int, help="Block number to check balances at"
    )
    parser.add_argument(
        "--accounts", nargs="+", help="List of account addresses to check"
    )
    parser.add_argument("--contract", required=True, help="Contract address")
    parser.add_argument(
        "--sum",
        action="store_true",
        help="Calculate and display the sum of all balances",
    )
    parser.add_argument(
        "--merchants", action="store_true", help="Check balances for merchant addresses"
    )
    parser.add_argument(
        "--merchants_csv",
        type=str,
        help="Path to CSV file containing merchant addresses",
    )

    args = parser.parse_args()

    w3 = AsyncWeb3(AsyncHTTPProvider(args.chain_url + "?app=balance_checker"))

    if not await w3.is_connected():
        print("Failed to connect to the Ethereum node.")
        return

    if args.date and args.block_number:
        print("Error: Provide either date or block_number, not both.")
        return

    if args.date:
        from datetime import datetime

        try:
            date_obj = datetime.strptime(args.date, "%d/%m/%Y")
            date_obj = date_obj.replace(hour=23, minute=59, second=59)
            timestamp = int(date_obj.timestamp())
        except ValueError:
            print("Error: Invalid date format. Please use DD/MM/YYYY.")
            return
        block_number = await find_closest_block_by_timestamp(w3, timestamp)
    elif args.block_number:
        block_number = args.block_number
    else:
        print("Error: Either date or block_number must be provided.")
        return

    print(f"Checking at block number: {block_number}")

    if args.merchants:
        if args.merchants_csv:
            merchant_addresses = get_merchants_csv(args.merchants_csv)
            balances = await get_balances(
                w3,
                args.contract,
                merchant_addresses,
                block_number,
                n=len(merchant_addresses),
            )
        else:
            merchant_addresses = get_merchants()
            balances = await get_balances(
                w3,
                args.contract,
                address_generator(merchant_addresses),
                block_number,
                n=cast(int, merchant_addresses.total_rows),
            )
    elif args.accounts:
        balances = await get_balances(
            w3, args.contract, args.accounts, block_number, n=len(args.accounts)
        )
    else:
        total_supply = await get_total_supply(w3, args.contract, block_number)
        print(f"Total supply: {total_supply}")
        return

    if args.sum:
        total_balance = aggregate_balances(balances)
        print(f"Total balance: {total_balance}")
    else:
        for account, balance in balances.items():
            if balance is not None:
                print(f"Address: {account}, Balance: {balance}")
            else:
                print(f"Address: {account}, Balance: Unable to retrieve")


def main():
    asyncio.run(async_main())


if __name__ == "__main__":
    main()
