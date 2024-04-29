from typing import List
from deepdiff import DeepDiff
from hexbytes import HexBytes
from pprintpp import pformat
import typer
import web3
from rich import print


def print_items(changes: list, ignore: list[str], indent: int = 0):
    for change in changes:
        field = change.path(root="", output_format="list")[0]
        if field not in ignore:
            print(f"{'\t'*indent}[bold red]{field}[/bold red]")
    print()


def print_changes(changes: list, ignore: list[str], left, right, indent: int = 0):
    for change in changes:
        field = change.path(root="", output_format="list")[0]
        if field not in ignore:
            if isinstance(left[field], HexBytes) and isinstance(right[field], HexBytes):
                print(
                    f"{'\t'*indent}{field}:\n{'\t'*indent}\t[bold red]Left: {left[field].hex()}[/bold red]\n{'\t'*indent}\t[bold green]Right: {right[field].hex()}[/bold green]"
                )
            else:
                print(
                    f"{'\t'*indent}{field}:\n{'\t'*indent}\t[bold red]Left: {left[field]}[/bold red]\n{'\t'*indent}\t[bold green]Right: {right[field]}[/bold green]"
                )
    print()


def print_diff(
    diff: DeepDiff,
    left,
    right,
    name: str,
    ignore: list[str],
    ignore_in_item: list[str],
    indent: int = 0,
):
    print(f"{'\t'*indent}[bold blue]{name} Diff:[/bold blue]")
    if len(diff.get("dictionary_item_added", [])):
        print(
            f"{'\t'*indent}\t[red]The left {name} is missing the following fields:[/red]"
        )
        print_items(diff["dictionary_item_added"], [*ignore, *ignore_in_item], indent + 2)

    if len(diff.get("dictionary_item_removed", [])):
        print(
            f"{'\t'*indent}\t[red]The right {name} is missing the following fields:[/red]"
        )
        print_items(diff["dictionary_item_removed"], [*ignore, *ignore_in_item], indent + 2)

    if len(diff.get("values_changed", [])):
        print(f"{'\t'*indent}\t[red]The follwing values don't match:[/red]")
        print_changes(diff["values_changed"], ignore, left, right, indent + 2)


def main(rpc_left: str, rpc_right: str, block_number: int, ignore: List[str] = []):
    """
    Compare a block's header, transactions, and transaction receipts between the two provided rpcs.

    If you want to ignore a some comparisons use the --ignore option. (eg. <command> --ignore logsBloom --ignore cumulativeGasUsed)
    """
    w3_left = web3.Web3(web3.HTTPProvider(rpc_left))
    w3_right = web3.Web3(web3.HTTPProvider(rpc_right))

    left_block = w3_left.eth.get_block(block_number, full_transactions=True)
    right_block = w3_right.eth.get_block(block_number, full_transactions=True)

    block_diff = DeepDiff(left_block, right_block, view="tree")

    print_diff(block_diff, left_block, right_block, "Block", ignore, ["transactions"])

    transactions_left = left_block.get("transactions")
    transactions_right = right_block.get("transactions")

    if len(transactions_left or []) != len(transactions_right or []):
        print(
            f"[red]Transactions length mismatch: [/red][bold red]Left: {len(transactions_left or [])}[/bold red], [bold green]Right: {len(transactions_right or [])}[/bold green]\n"
        )

    if transactions_left is not None and transactions_right is not None:
        for idx, (left_tx, right_tx) in enumerate(
            zip(transactions_left, transactions_right)
        ):
            tx_diff = DeepDiff(left_tx, right_tx, view="tree")
            print_diff(tx_diff, left_tx, right_tx, f"Transaction {idx}", ignore, [], 1)

            if not isinstance(left_tx, HexBytes) and not isinstance(right_tx, HexBytes):
                left_receipt = w3_left.eth.get_transaction_receipt(
                    right_tx.get("hash", HexBytes(""))
                )
                right_receipt = w3_right.eth.get_transaction_receipt(
                    right_tx.get("hash", HexBytes(""))
                )

                receipt_diff = DeepDiff(left_receipt, right_receipt, view="tree")
                print_diff(
                    receipt_diff,
                    left_receipt,
                    right_receipt,
                    f"Transaction {idx} Receipt",
                    ignore,
                    ["logs"],
                    2,
                )


if __name__ == "__main__":
    typer.run(main)
