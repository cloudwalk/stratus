#!/bin/bash
#
# Clone Git repositories containing Solidity contracts.
#
source $(dirname $0)/_functions.sh

# ------------------------------------------------------------------------------
# Functions
# ------------------------------------------------------------------------------

# Clone a project to the projects directory.
clone() {
    repo=$1
    target=repos/$repo

    mkdir -p repos

    if [ -d $target ]; then
        log "Updating: $repo"
        git -C $target pull
    else
        log "Cloning: $repo"
        # git clone git@github.com:cloudwalk/$repo.git $target
        git clone https://github.com/cloudwalk/$repo.git $target
    fi

    log "Installing dependencies: $repo"
    npm --prefix $target --silent install
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

log "Cloning or updating repositories"
clone brlc-multisig
clone brlc-periphery
clone brlc-token
clone compound-periphery
clone brlc-yield-streamer
clone brlc-pix-cashier