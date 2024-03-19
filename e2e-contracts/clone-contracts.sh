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
    commit=$2
    target=repos/$repo

    mkdir -p repos

    if [ -d $target ]; then
        log "Updating: $repo"
        git -C $target pull
    else
        log "Cloning: $repo"
        # git clone git@github.com:cloudwalk/$repo.git $target
        git clone https://github.com/cloudwalk/$repo.git $target
        
        # checkout commit if specified and it's different from HEAD
        head_commit=$(git -C $target rev-parse --short HEAD)
        if [ -n "$commit" ] && [ "$commit" != "$head_commit" ]; then
            log "Checking out commit: $commit"
            git -C $target checkout $commit --quiet
        fi
    fi

    log "Installing dependencies: $repo"
    npm --prefix $target --silent install
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

log "Cloning or updating repositories"
clone brlc-multisig       918a226
clone brlc-periphery      b8d507a
clone brlc-token          0858ec4
clone compound-periphery  e4d68df
clone brlc-yield-streamer e63d8ba
clone brlc-pix-cashier    00cc007