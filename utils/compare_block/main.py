from typing import List
from deepdiff import DeepDiff
from hexbytes import HexBytes
from pprintpp import pformat
import typer
import web3
from rich import print

def main(rpc_left: str, rpc_right: str, block: int, ignore: List[str] = []):
    w3_left = web3.Web3(web3.HTTPProvider(rpc_left))
    w3_right = web3.Web3(web3.HTTPProvider(rpc_right))

    block_left = w3_left.eth.get_block(block, full_transactions=True)
    block_right = w3_right.eth.get_block(block, full_transactions=True)

    block_diff = DeepDiff(block_left, block_right, view='tree')

    print("Block Header Diff:")
    if len(block_diff['dictionary_item_added']):
        print("\t[red]The left block is missing the following fields:[/red]")
        for change in block_diff['dictionary_item_added']:
            field = change.path(root="", output_format="list")[0]
            if field != "transactions" and field not in ignore:
                print(f"\t\t[bold red]{field}[/bold red]")
        print()


    if len(block_diff['dictionary_item_removed']):
        print("\t[red]The right block is missing the following fields:[/red]")
        for change in block_diff['dictionary_item_removed']:
            field = change.path(root="", output_format="list")[0]
            if field != "transactions" and field not in ignore:
                print(f"[bold red]\t\t{field}[/bold red]")
        print()

    if len(block_diff['values_changed']):
        print("\t[red]The follwing values don't match:[/red]")
        for change in block_diff['values_changed']:
            field = change.path(root="", output_format="list")[0]
            if field != "transactions" and field not in ignore:
                if isinstance(block_left[field], HexBytes) and isinstance(block_right[field], HexBytes):
                    print(f"\t\t{field}: [bold red]Left: {block_left[field].hex()}[/bold red], [bold green]Right: {block_right[field].hex()}[/bold green]")
                else:
                    print(f"\t\t{field}: [bold red]Left: {block_left[field]}[/bold red], [bold green]Right: {block_right[field]}[/bold green]")
        print()


    transactions_left = block_left.get('transactions')
    transactions_right = block_right.get('transactions')

    if transactions_left is not None and transactions_right is not None:
        if len(transactions_left) != len(transactions_right):
            print(f"[red]Transactions length mismatch: [/red][bold red]Left: {len(transactions_left)}[/bold red], [bold green]Right: {len(transactions_right)}[/bold green]")
            print()

        for (idx, (left, right)) in enumerate(zip(transactions_left, transactions_right)):
            print(f"\tTransaction {idx} Diff:")
            diff = DeepDiff(left, right, view='tree')

            if len(diff.get('dictionary_item_added', [])):
                print("\t\t[red]The left tx is missing the following fields:[/red]")
                for change in diff['dictionary_item_added']:
                    field = change.path(root="", output_format="list")[0]
                    if field not in ignore:
                        print(f"\t\t\t[bold red]{field}[/bold red]")
                print()


            if len(diff.get('dictionary_item_removed', [])):
                print("\t\t[red]The right tx is missing the following fields:[/red]")
                for change in diff['dictionary_item_removed']:
                    field = change.path(root="", output_format="list")[0]
                    if field not in ignore:
                        print(f"[bold red]\t\t\t{field}[/bold red]")
                print()

            if len(diff.get('values_changed', [])):
                print("\t\t[red]The follwing values don't match:[/red]")
                for change in diff['values_changed']:
                    field = change.path(root="", output_format="list")[0]
                    if field not in ignore:
                        if isinstance(left[field], HexBytes) and isinstance(right[field], HexBytes):
                            print(f"\t\t\t{field}: [bold red]Left: {left[field].hex()}[/bold red], [bold green]Right: {right[field].hex()}[/bold green]")
                        else:
                            print(f"\t\t\t{field}: [bold red]Left: {left[field]}[/bold red], [bold green]Right: {right[field]}[/bold green]")
                print()

            if not isinstance(left, HexBytes) and not isinstance(right, HexBytes):
                print(f"\tTransaction {idx} Receipt Diff:")

                left = w3_left.eth.get_transaction_receipt(left.get("hash", HexBytes("")))
                right = w3_right.eth.get_transaction_receipt(right.get("hash", HexBytes("")))

                diff = DeepDiff(left, right, view='tree')

                if len(diff.get('dictionary_item_added', [])):
                    print("\t\t[red]The left tx receipt is missing the following fields:[/red]")
                    for change in diff['dictionary_item_added']:
                        field = change.path(root="", output_format="list")[0]
                        if field != "logs" and field not in ignore:
                            print(f"\t\t\t[bold red]{field}[/bold red]")
                    print()


                if len(diff.get('dictionary_item_removed', [])):
                    print("\t\t[red]The right tx receipt is missing the following fields:[/red]")
                    for change in diff['dictionary_item_removed']:
                        field = change.path(root="", output_format="list")[0]
                        if field != "logs" and field not in ignore:
                            print(f"[bold red]\t\t\t{field}[/bold red]")
                    print()

                if len(diff.get('values_changed', [])):
                    print("\t\t[red]The follwing values don't match:[/red]")
                    for change in diff['values_changed']:
                        field = change.path(root="", output_format="list")[0]
                        if field not in ignore:
                            if isinstance(left[field], HexBytes) and isinstance(right[field], HexBytes):
                                print(f"\t\t\t{field}: [bold red]Left: {left[field].hex()}[/bold red], [bold green]Right: {right[field].hex()}[/bold green]")
                            else:
                                print(f"\t\t\t{field}: [bold red]Left: {pformat(left[field], indent=8)}[/bold red], [bold green]Right: {pformat(right[field], indent=8)}[/bold green]")
                    print()


if __name__ == "__main__":
    typer.run(main)
