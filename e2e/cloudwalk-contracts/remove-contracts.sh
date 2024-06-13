#!/bin/bash
#
# Clone Git repositories containing Solidity contracts.
#
source $(dirname $0)/_functions.sh

# ------------------------------------------------------------------------------
# Functions
# ------------------------------------------------------------------------------

# Clone a project to the projects directory.
remove() {
    repo=$1
    target=repos/$repo

    if [ -d $target ]; then
        log "Removing: $repo"
        rm -rf $target
    else
        log "Already removed: $repo"
        log "Nothing to do, leaving"
    fi
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

log "Removing repositories"
remove brlc-multisig
remove brlc-periphery
remove brlc-token
remove compound-periphery
remove brlc-yield-streamer
remove brlc-pix-cashier