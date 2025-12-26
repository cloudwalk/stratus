#!/bin/bash
#
# Clone Git repositories containing Solidity contracts.
#
set -eo pipefail
source "$(dirname "$0")/_functions.sh"

# ------------------------------------------------------------------------------
# Functions
# ------------------------------------------------------------------------------

# Clone a project to the projects directory.
clone() {
    repo=brlc-monorepo
    target=../brlc-monorepo

    if [ -d "$target" ]; then
        log "Updating: $repo"
        git -C "$target" pull
    else
        log "Cloning: $repo"
        # TODO: remove the stratus-integration branch once the integration tests are merged into main
        if ! git clone https://github.com/cloudwalk/"$repo".git -b stratus-integration "$target"; then
            log "Clone failed. Removing folder and exiting."
            rm -rf "$target"
            return 1
        fi
    fi

    log "Installing dependencies: $repo"
    if ! pnpm -C "$target" install; then
        log "Dependencies install failed. Removing folder and exiting."
        rm -rf "$target"
        return 1
    fi
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

log "Cloning or updating brlc-monorepo"

clone
