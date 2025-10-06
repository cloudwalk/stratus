#!/bin/bash
#
# Generate coverage info for the Solidity contracts.
#
set -eo pipefail
source "$(dirname "$0")/_functions.sh"

# ------------------------------------------------------------------------------
# Functions
# ------------------------------------------------------------------------------

# Generate a solidity coverage info.
coverage() {
    repo=$1

    if [ -d repos/"$repo"/coverage/ ]; then
        log "Already generated coverage for $repo"
        return
    fi

    log "Generating coverage: $repo"

    # Enter the repository folder
    if [ ! -d repos/"$repo" ]; then
        log "Repository not found: $repo. Is it cloned?"
        return 1
    fi

    # shellcheck disable=SC2164
    # reason: the existence of the repository is checked above
    cd repos/"$repo"

    npx hardhat coverage

    # Leave the repository folder
    cd ../../
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

# configure tools
asdf local solidity 0.8.16 || echo "asdf, solidity plugin or solidity version not found"

# execute
coverage brlc-token

# Periphery Transition: calculate coverage regardless of the repository's current name
coverage brlc-card-payment-processor || coverage brlc-periphery

# Cashier Transition: calculate coverage regardless of the repository's current name
coverage brlc-cashier

coverage brlc-balance-tracker || log "Balance Tracker is not isolated yet. Skipping..."
coverage brlc-net-yield-distributor
coverage brlc-multisig
coverage brlc-capybara-finance
coverage brlc-credit-agent

if [ -n "$1" ] && [ "$1" = "--html" ]; then
    log "Opening coverage reports in your web browser..."
    open repos/*/coverage/index.html
fi
