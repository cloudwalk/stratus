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
    log "Flattenning: $contract ($repo)"

    # Enter the repository folder
    cd repos/$repo

    # Flatten
    npx hardhat flatten contracts/$contract.sol > ../../integration/contracts/$contract.flattened.sol
    
    # Leave the repository folder
    cd ../../
    
    # Lint the flattened contract
    log "Linting the flattened $contract ($repo)"
    npx ts-node integration/lintFlattened.ts integration/contracts/$contract.flattened.sol
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

# configure tools
asdf local solidity 0.8.16

# execute
flatten brlc-token          BRLCToken
flatten brlc-periphery      CardPaymentProcessor
flatten brlc-periphery      CashbackDistributor
flatten brlc-pix-cashier    PixCashier
flatten brlc-multisig       MultiSigWallet
flatten compound-periphery  CompoundAgent