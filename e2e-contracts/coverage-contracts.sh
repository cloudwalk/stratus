#!/bin/bash
#
# Generate coverage info for the Solidity contracts.
#
source $(dirname $0)/_functions.sh

# ------------------------------------------------------------------------------
# Functions
# ------------------------------------------------------------------------------

# Generate a solidity coverage info.
coverage() {
    repo=$1

    if [ -d repos/$repo/coverage/ ]; then
        log "Already generated coverage for $repo"
        return
    fi

    log "Generating coverage: $repo"

    # Enter the repository folder
    cd repos/$repo
    
    npx hardhat coverage
    
    # Leave the repository folder
    cd ../../
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

# configure tools
asdf local solidity 0.8.16

# execute
coverage brlc-token     
coverage brlc-periphery    
coverage brlc-pix-cashier
coverage brlc-yield-streamer
coverage brlc-multisig
coverage compound-periphery

if [ -n "$1" ] && [ "$1" = "--html" ]; then
    log "Opening coverage reports in your web browser..."
    open repos/*/coverage/index.html
fi