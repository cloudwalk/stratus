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

# Initialize variables
token=0
periphery=0
multisig=0
compound=0
yield=0
pix=0

# Help function
print_help() {
    echo "Usage: $0 [OPTIONS]"
    echo "Options:"
    echo "  -t, --token       for brlc-token"
    echo "  -p, --periphery   for brlc-periphery"
    echo "  -m, --multisig    for brlc-multisig"
    echo "  -c, --compound    for compound-periphery"
    echo "  -i, --yield       for brlc-yield-streamer"
    echo "  -x, --pix         for brlc-pix-cashier"
    echo "  -h, --help        display this help and exit"
}

if [ "$#" == 0 ]; then
    token=1
    periphery=1
    multisig=1
    compound=1
    yield=1
    pix=1
fi

# Process arguments
while [[ "$#" -gt 0 ]]; do
    case "$1" in
        -h|--help) print_help; exit 0 ;;
        -t|--token) token=1; shift ;;
        -p|--periphery) periphery=1; shift ;;
        -m|--multisig) multisig=1; shift ;;
        -c|--compound) compound=1; shift ;;
        -i|--yield) yield=1; shift ;;
        -x|--pix) pix=1; shift ;;
        *) echo "Unknown option: $1"; print_help; exit 1 ;;
    esac
done

log "Removing repositories"

if [ "$token" == 1 ]; then
    remove brlc-token 80bdba3
fi

if [ "$pix" == 1 ]; then
    remove brlc-pix-cashier a528d0c
fi

if [ "$yield" == 1 ]; then
    remove brlc-yield-streamer 7683517
fi

if [ "$periphery" == 1 ]; then
    remove brlc-periphery fed9fcb
fi

if [ "$multisig" == 1 ]; then
    remove brlc-multisig 918a226
fi

if [ "$compound" == 1 ]; then
    remove compound-periphery e4d68df
fi