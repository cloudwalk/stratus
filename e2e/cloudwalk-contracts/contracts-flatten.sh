#!/bin/bash
#
# Flattens a subset or relevant Solidity contracts.
#
set -eo pipefail
source "$(dirname "$0")/_functions.sh"
root=$(pwd)

# ------------------------------------------------------------------------------
# Functions
# ------------------------------------------------------------------------------

# Flatten Solidity contracts from a project.
flatten() {
    contract_project=$1
    contract=$2
    target_file="$root/integration/contracts/$contract.flattened.sol"

    rm -rf ../node_modules # TODO: remove


    echo "root: $root"
    echo "contract: $contract"
    echo "contract_project: $contract_project"

    if [ -f "$target_file" ]; then
        log "Skipping flattening of $contract ($contract_project)"
        return
    fi

    log "Flattening: $contract ($contract_project)"

    # Flatten

    pnpm -C ../brlc-monorepo run flatten:contract "$contract_project" "$contract".sol $target_file

    # Leave the repository folder
    cd $root

    # Install ts-node if not installed
    command -v ts-node >/dev/null || npm install --save-dev ts-node

    # Lint the flattened contract
    log "Linting the flattened $contract ($contract_project)"
 #   npx ts-node integration/test/helpers/lint-flattened.ts integration/contracts/"$contract".flattened.sol

}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

# Help function
print_help() {
    echo "Usage: $0 <project_name> <contract_name>"
    echo ""
    echo "Flattens a single Solidity contract from a given project in the brlc-monorepo into integration/contracts."
    echo ""
    echo "Arguments:"
    echo "  project_name   Folder name under brlc-monorepo/ (e.g. token)"
    echo "  contract_name  Solidity contract file name without .sol (e.g. BRLCToken)"
}

case "${1:-}" in
    -h | --help)
        print_help
        exit 0
        ;;
esac

if [ "$#" -ne 2 ]; then
    echo "Error: expected <project_name> and <contract_name>."
    echo ""
    print_help
    exit 1
fi

project_name=$1
contract_name=$2

log "Flattening contract"
flatten "$project_name" "$contract_name"
