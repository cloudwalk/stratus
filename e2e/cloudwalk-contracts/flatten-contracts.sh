#!/bin/bash
#
# Flattens a subset or relevant Solidity contracts.
#
source $(dirname $0)/_functions.sh

# ------------------------------------------------------------------------------
# Functions
# ------------------------------------------------------------------------------

# Flatten Solidity contracts from a project.
flatten() {
    repo=$1
    contract=$2

    if [ -f integration/contracts/$contract.flattened.sol ]; then
        echo "Skipping flattening of $contract ($repo)"
        return
    fi

    log "Flattenning: $contract ($repo)"

    # Enter the repository folder
    cd repos/$repo
    cp ../../../hardhat.config.ts .
    
    # Flatten
    npx hardhat flatten contracts/$contract.sol > ../../integration/contracts/$contract.flattened.sol
    
    # Leave the repository folder
    cd ../../

    # Install ts-node if not installed
    command -v ts-node > /dev/null || npm install --save-dev ts-node
    
    # Lint the flattened contract
    log "Linting the flattened $contract ($repo)"
    npx ts-node integration/test/helpers/lint-flattened.ts integration/contracts/$contract.flattened.sol
    
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

# Initialize variables
token=0
periphery=0
multisig=0
compound=0
yield=0
pix=0

# Help function
print_help() {
    echo "Usage: $0 [OPTIONS]"
    echo "Options:"
    echo "  -t, --token       for brlc-token"
    echo "  -p, --periphery   for brlc-periphery"
    echo "  -m, --multisig    for brlc-multisig"
    echo "  -c, --compound    for compound-periphery"
    echo "  -i, --yield       for brlc-yield-streamer"
    echo "  -x, --pix         for brlc-pix-cashier"
    echo "  -h, --help        display this help and exit"
}

if [ "$#" == 0 ]; then
    token=1
    periphery=1
    multisig=1
    compound=1
    yield=1
    pix=1
fi

# Process arguments
while [[ "$#" -gt 0 ]]; do
    case "$1" in
        -h|--help) print_help; exit 0 ;;
        -t|--token) token=1; shift ;;
        -p|--periphery) periphery=1; shift ;;
        -m|--multisig) multisig=1; shift ;;
        -c|--compound) compound=1; shift ;;
        -i|--yield) yield=1; shift ;;
        -x|--pix) pix=1; shift ;;
        *) echo "Unknown option: $1"; print_help; exit 1 ;;
    esac
done
# configure tools
asdf local solidity 0.8.16 || echo "asdf, solidity plugin or solidity version not found"

log "Flattening repositories"

if [ "$token" == 1 ]; then
    flatten brlc-token          BRLCToken
fi

if [ "$pix" == 1 ]; then
    flatten brlc-pix-cashier    PixCashier
fi

if [ "$yield" == 1 ]; then
    flatten brlc-yield-streamer BalanceTracker
    flatten brlc-yield-streamer YieldStreamer
fi

if [ "$periphery" == 1 ]; then
    flatten brlc-periphery      CardPaymentProcessor
    flatten brlc-periphery      CashbackDistributor
fi

if [ "$multisig" == 1 ]; then
    flatten brlc-multisig       MultiSigWallet
fi

if [ "$compound" == 1 ]; then
    flatten compound-periphery  CompoundAgent
fi