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

# Clone an alternative version of a project to the projects directory.
clone_alternative() {
    repo=$1
    commit=$2
    branch=$3
    folder=$4
    target=repos/$folder

    mkdir -p repos

    if [ -d $target ]; then
        log "Updating: $repo in $folder"
        git -C $target pull
    else
        log "Cloning: $branch branch of $repo in $folder"
        git clone --branch $branch https://github.com/cloudwalk/$repo.git $target
        
        # checkout commit if specified and it's different from HEAD
        head_commit=$(git -C $target rev-parse --short HEAD)
        if [ -n "$commit" ] && [ "$commit" != "$head_commit" ]; then
            log "Checking out commit: $commit"
            git -C $target checkout $commit --quiet
        fi
    fi

    log "Installing dependencies: $folder"
    npm --prefix $target --silent install
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
pixv3=0
pixv4=0
cppv2=0

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
    echo "  -3, --pixv3       for brlc-pix-cashier-v3"
    echo "  -4, --pixv4       for brlc-pix-cashier-v4"
    echo "  -2, --cppv2       for brlc-periphery-v2"
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
        -3|--pixv3) pixv3=1; shift ;;
        -4|--pixv4) pixv4=1; shift ;;
        -2|--cppv2) cppv2=1; shift ;;
        *) echo "Unknown option: $1"; print_help; exit 1 ;;
    esac
done

log "Cloning or updating repositories"

if [ "$token" == 1 ]; then
    clone brlc-token 80bdba3
fi

if [ "$pix" == 1 ]; then
    clone brlc-pix-cashier fe9343c
fi

if [ "$yield" == 1 ]; then
    clone brlc-yield-streamer 236dbcb
fi

if [ "$periphery" == 1 ]; then
    clone brlc-periphery fed9fcb
fi

if [ "$multisig" == 1 ]; then
    clone brlc-multisig 918a226
fi

if [ "$compound" == 1 ]; then
    clone compound-periphery c3ca5df
fi

# Alternative versions

if [ "$pixv3" == 1 ]; then
    clone_alternative brlc-pix-cashier 2b1e7b3 pix-cashier-v3 brlc-pix-cashier-v3
fi

if [ "$pixv4" == 1 ]; then
    clone_alternative brlc-pix-cashier 3ccc6c4 pix-cashier-v4 brlc-pix-cashier-v4
fi

if [ "$cppv2" == 1 ]; then
    clone_alternative brlc-periphery 1e5344f cpp2 brlc-periphery-v2
fi