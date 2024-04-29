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
            print(f"{'\t'*indent}\t\t[bold red]{field}[/bold red]")
    print()

def print_changes(changes: list, ignore: list[str], left, right, indent: int = 0):
    for change in changes:
        field = change.path(root="", output_format="list")[0]
        if field not in ignore:
            if isinstance(left[field], HexBytes) and isinstance(right[field], HexBytes):
                print(f"{'\t'*indent}\t\t{field}:\n{'\t'*indent}\t\t\t[bold red]Left: {left[field].hex()}[/bold red]\n{'\t'*indent}\t\t\t[bold green]Right: {right[field].hex()}[/bold green]")
            else:
                print(f"{'\t'*indent}\t\t{field}:\n{'\t'*indent}\t\t\t[bold red]Left: {left[field]}[/bold red]\n{'\t'*indent}\t\t\t[bold green]Right: {right[field]}[/bold green]")
    print()

def print_diff(diff: DeepDiff, left, right, name: str, ignore: list[str], ignore_in_item: list[str], indent: int = 0):
    print(f"{'\t'*indent}{name} Diff:")
    if len(diff.get('dictionary_item_added', [])):
        print(f"{'\t'*indent}\t[red]The left {name} is missing the following fields:[/red]")
        print_items(diff['dictionary_item_added'], [*ignore, *ignore_in_item], indent)

    if len(diff.get('dictionary_item_removed', [])):
        print(f"{'\t'*indent}\t[red]The right {name} is missing the following fields:[/red]")
        print_items(diff['dictionary_item_removed'], [*ignore, *ignore_in_item], indent)

    if len(diff.get('values_changed', [])):
        print(f"{'\t'*indent}\t[red]The follwing values don't match:[/red]", indent)
        print_changes(diff['values_changed'], ignore, left, right)

def main(rpc_left: str, rpc_right: str, block: int, ignore: List[str] = []):
    w3_left = web3.Web3(web3.HTTPProvider(rpc_left))
    w3_right = web3.Web3(web3.HTTPProvider(rpc_right))

    block_left = w3_left.eth.get_block(block, full_transactions=True)
    block_right = w3_right.eth.get_block(block, full_transactions=True)

    block_diff = DeepDiff(block_left, block_right, view='tree')

    print_diff(block_diff, block_left, block_right, "Block Header", ignore, ["transactions"])

    transactions_left = block_left.get('transactions')
    transactions_right = block_right.get('transactions')
    if transactions_left is not None and transactions_right is not None:
        if len(transactions_left) != len(transactions_right):
            print(f"[red]Transactions length mismatch: [/red][bold red]Left: {len(transactions_left)}[/bold red], [bold green]Right: {len(transactions_right)}[/bold green]")
            print()

        for (idx, (left, right)) in enumerate(zip(transactions_left, transactions_right)):
            tx_diff = DeepDiff(left, right, view='tree')
            print_diff(tx_diff, left, right, f"Transaction {idx}", ignore, [], 1)

            if not isinstance(left, HexBytes) and not isinstance(right, HexBytes):
                left_receipt = w3_left.eth.get_transaction_receipt(left.get("hash", HexBytes("")))
                right_receipt = w3_right.eth.get_transaction_receipt(right.get("hash", HexBytes("")))

                receipt_diff = DeepDiff(left_receipt, right_receipt, view='tree')
                print_diff(receipt_diff, left_receipt, right_receipt, f"Transaction {idx} Receipt", ignore, ["logs"], 2)


if __name__ == "__main__":
    typer.run(main)
