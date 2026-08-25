#!/bin/bash
#
# Runs tests for Solidity contracts.
#
set -eo pipefail
source "$(dirname "$0")/_functions.sh"

# ------------------------------------------------------------------------------
# Functions
# ------------------------------------------------------------------------------
test() {
    log "Testing: $1"

    # configure hardhat env
    if ! cd ../brlc-monorepo; then
        log "Error: brlc-monorepo not found"
        return 1
    fi
    cd ../brlc-monorepo/contracts/$1

    pnpm run --if-present test:stratus

    result_code=$?

    # exit with same return code as the test if an error ocurred
    if [ $result_code -ne 0 ]; then
        exit $result_code
    fi
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------c

# Help function
print_help() {
    echo "Usage: $0 [ CONTRACT ]"
    echo "Contract from brlc-monorepo/contracts"
}

test $1
