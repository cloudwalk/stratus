#!/bin/bash
#
# Compiles a subset or relevant Solidity contracts.
#
set -eo pipefail
source "$(dirname "$0")/_functions.sh"

# ------------------------------------------------------------------------------
# Functions
# ------------------------------------------------------------------------------

# Compile Solidity contracts from a project.
compile_contract() {
    repo=$1
    contract=$2
    log "Compiling: $contract ($repo)"

    # compile
    solc --base-path repos/"$repo"/contracts --include-path repos/"$repo"/node_modules --hashes --optimize -o repos/"$repo"/target --overwrite repos/"$repo"/contracts/"$contract".sol

    # copy from target folder to tests
    cp repos/"$repo"/target/"$contract".signatures ../../static/contracts-signatures/
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

# configure tools
asdf local solidity 0.8.24 || echo "asdf, solidity plugin or solidity version not found"

# execute
compile_contract brlc-token BRLCToken

# Cashier Transition: compile both Cashier and CashierShard regardless if the repository was renamed or not
compile_contract brlc-cashier Cashier || compile_contract brlc-pix-cashier Cashier
compile_contract brlc-cashier CashierShard || compile_contract brlc-pix-cashier CashierShard

# Periphery Transition: compile both Periphery and PeripheryShard regardless if the repository was renamed or not
compile_contract brlc-card-payment-processor CashbackDistributor || compile_contract brlc-periphery CashbackDistributor
compile_contract brlc-card-payment-processor CardPaymentProcessor || compile_contract brlc-periphery CardPaymentProcessor

compile_contract compound-periphery CompoundAgent

compile_contract brlc-multisig MultiSigWallet

compile_contract brlc-yield-streamer BalanceTracker
compile_contract brlc-yield-streamer YieldStreamer
