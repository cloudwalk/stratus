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
    repo=$1
    commit=$2
    target=repos/$repo

    mkdir -p repos

    if [ -d "$target" ]; then
        log "Updating: $repo"
        git -C "$target" pull
    else
        log "Cloning: $repo"
        if ! git clone https://github.com/cloudwalk/"$repo".git "$target"; then
            log "Clone failed. Removing folder and exiting."
            rm -rf "$target"
            return 1
        fi

        # checkout commit if specified and it's different from HEAD
        head_commit=$(git -C "$target" rev-parse --short HEAD)
        if [ -n "$commit" ] && [ "$commit" != "$head_commit" ]; then
            log "Checking out commit: $commit"
            git -C "$target" checkout "$commit" --quiet
        fi
    fi

    log "Installing dependencies: $repo"
    if ! npm --prefix "$target" install; then
        log "Dependencies install failed. Removing folder and exiting."
        rm -rf "$target"
        return 1
    fi
}

# Clone an alternative version of a project to the projects directory.
clone_alternative() {
    repo=$1
    branch=$2
    folder=$3
    commit=$4
    target=repos/$folder

    mkdir -p repos

    if [ -d "$target" ]; then
        log "Updating: $repo in $folder"
        git -C "$target" pull
    else
        log "Cloning: $branch branch of $repo in $folder"
        if ! git clone --branch "$branch" https://github.com/cloudwalk/"$repo".git "$target"; then
            log "Clone failed. Removing folder and exiting."
            rm -rf "$target"
            return 1
        fi
        # checkout commit if specified and it's different from HEAD
        head_commit=$(git -C "$target" rev-parse --short HEAD)
        if [ -n "$commit" ] && [ "$commit" != "$head_commit" ]; then
            log "Checking out commit: $commit"
            git -C "$target" checkout "$commit" --quiet
        fi
    fi

    log "Installing dependencies: $folder"
    npm --prefix "$target" install
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

# Initialize variables
token=0
periphery=0
multisig=0
yield=0
pix=0
cppv2=0
capybara_finance=0
credit_agent=0

# Help function
print_help() {
    echo "Usage: $0 [OPTIONS]"
    echo "Options:"
    echo "  -t, --token             for brlc-token"
    echo "  -p, --periphery         for brlc-periphery"
    echo "  -m, --multisig          for brlc-multisig"
    echo "  -i, --yield             for brlc-net-yield-distributor"
    echo "  -x, --pix               for brlc-cashier"
    echo "  -2, --cppv2             for brlc-periphery-v2"
    echo "  -f, --capybara-finance  for brlc-capybara-finance"
    echo "  -a, --credit-agent      for brlc-credit-agent"
    echo "  -h, --help              display this help and exit"
}

if [ "$#" == 0 ]; then
    token=1
    periphery=1
    multisig=1
    yield=1
    pix=1
    cppv2=1
    capybara_finance=1
    credit_agent=1
fi

# Process arguments
while [[ "$#" -gt 0 ]]; do
    case "$1" in
    -h | --help)
        print_help
        exit 0
        ;;
    -t | --token)
        token=1
        shift
        ;;
    -p | --periphery)
        periphery=1
        shift
        ;;
    -m | --multisig)
        multisig=1
        shift
        ;;
    -i | --yield)
        yield=1
        shift
        ;;
    -x | --pix)
        pix=1
        shift
        ;;
    -2 | --cppv2)
        cppv2=1
        shift
        ;;
    -f | --capybara-finance)
        capybara_finance=1
        shift
        ;;
    -a | --credit-agent)
        credit_agent=1
        shift
        ;;
    *)
        echo "Unknown option: $1"
        print_help
        exit 1
        ;;
    esac
done

log "Cloning or updating repositories"

if [ "$token" == 1 ]; then
    clone brlc-token
fi

if [ "$pix" == 1 ]; then
    # Cashier Transition: clone the 'brlc-cashier' repo using the default branch
    clone brlc-cashier
fi

if [ "$yield" == 1 ]; then
    # BalanceTracker Transition: clone balance tracker if it exists and contains the hardhat project.
    clone brlc-balance-tracker || log "Balance Tracker not isolated yet. Skipping..."
    clone brlc-net-yield-distributor
fi

if [ "$periphery" == 1 ]; then
    # Periphery Transition: attempts to clone the periphery repository/contract using different methods
    clone brlc-card-payment-processor || clone brlc-periphery
fi

if [ "$multisig" == 1 ]; then
    clone brlc-multisig
fi

if [ "$capybara_finance" == 1 ]; then
    clone brlc-capybara-finance
fi

if [ "$credit_agent" == 1 ]; then
    clone brlc-credit-agent
fi

# Alternative versions

if [ "$cppv2" == 1 ]; then
    # Periphery Transition: attempts to clone the periphery v2 repository/contract using different methods
    clone brlc-card-payment-processor-v2
fi
