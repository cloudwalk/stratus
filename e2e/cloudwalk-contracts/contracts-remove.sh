#!/bin/bash
#
# Remove cloned Git repositories containing CW Solidity contracts.
#
set -eo pipefail
source "$(dirname "$0")/_functions.sh"

# ------------------------------------------------------------------------------
# Functions
# ------------------------------------------------------------------------------

# Remove the brlc-monorepo clone (this is what `contracts-clone.sh` creates).
remove_brlc_monorepo() {
    target=../brlc-monorepo

    if [ -d "$target" ]; then
        log "Removing: brlc-monorepo ($target)"
        rm -rf "$target"
    else
        log "Already removed: brlc-monorepo ($target)"
        log "Nothing to do, leaving"
    fi
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

# Help function
print_help() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Removes the cloned brlc-monorepo repository used by the E2E contracts tests."
    echo ""
    echo "Options:"
    echo "  -h, --help  display this help and exit"
}

case "${1:-}" in
-h | --help)
    print_help
    exit 0
    ;;
"")
    # default: remove everything that was cloned
    ;;
*)
    echo "Unknown option: $1"
    print_help
    exit 1
    ;;
esac

log "Removing cloned repositories"
remove_brlc_monorepo
