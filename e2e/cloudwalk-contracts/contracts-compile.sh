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
    version=$3

    log "Compiling: $contract ($repo)"
    asdf plugin add solidity
    asdf install solidity $version
    asdf set solidity $version
    # compile
    asdf exec solc --base-path repos/"$repo"/contracts --include-path repos/"$repo"/node_modules --hashes --optimize -o repos/"$repo"/target --overwrite repos/"$repo"/contracts/"$contract".sol

    # copy from target folder to tests
    cp repos/"$repo"/target/"$contract".signatures ../../static/contracts-signatures/
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

# execute
compile_contract brlc-token BRLCToken 0.8.24

# Cashier Transition: compile both Cashier and CashierShard regardless if the repository was renamed or not
compile_contract brlc-cashier Cashier 0.8.24
compile_contract brlc-cashier CashierShard 0.8.24

# Periphery Transition: compile both Periphery and PeripheryShard regardless if the repository was renamed or not
compile_contract brlc-card-payment-processor CashbackDistributor 0.8.24
compile_contract brlc-card-payment-processor CardPaymentProcessor 0.8.24

compile_contract brlc-multisig MultiSigWallet 0.8.24

# BalanceTracker Transition: compile BalanceTracker regardless of the repository it is in.
compile_contract brlc-balance-tracker BalanceTracker 0.8.16

compile_contract brlc-net-yield-distributor YieldStreamer 0.8.16

# Capybara Finance contracts
compile_contract brlc-capybara-finance LendingMarket 0.8.24
compile_contract brlc-capybara-finance LiquidityPool 0.8.24
compile_contract brlc-capybara-finance CreditLine 0.8.24

# Credit Agent contracts
compile_contract brlc-credit-agent CreditAgent 0.8.24
