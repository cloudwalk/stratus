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
    repo=$1
    file=$2
    test=$3
    log "Testing: $file ($repo)"

    # configure hardhat env
    if ! cd repos/"$repo"; then
        log "Error: $repo not found"
        return 1
    fi
    git restore .
    cp ../../../hardhat.config.ts .
    rm -rf .openzeppelin/

    # apply git patch
    patch_file="../../patches/$repo.patch"
    if [ -e "$patch_file" ]; then
        log "Applying Git patch: $patch_file"
        git apply "$patch_file" || true
    fi

    # test
    if [ -z "$test" ]; then
        log "Executing all tests"
        npx hardhat test --bail --network stratus test/"$file".test.ts
    else
        log "Executing filtered tests: $test"
        npx hardhat test --bail --network stratus test/"$file".test.ts --grep "$test"
    fi
    result_code=$?

    # restore original files
    git restore .

    # go back to previous directory
    # shellcheck disable=SC2164
    # reason: This cd should not fail
    cd -

    # exit with same return code as the test if an error ocurred
    if [ $result_code -ne 0 ]; then
        exit $result_code
    fi
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

# configure tools
asdf local nodejs 20.10.0 || echo "asdf, nodejs plugin or nodejs version not found"

# Initialize variables
token=0
periphery=0
multisig=0
compound=0
yield=0
pix=0
pixv4=0
cppv2=0

# Help function
print_help() {
    echo "Usage: $0 [ CONTRACT ] [ <TEST_NAME> ]"
    echo "Contracts:"
    echo "  -t, --token       for brlc-token"
    echo "  -p, --periphery   for brlc-periphery"
    echo "  -m, --multisig    for brlc-multisig"
    echo "  -c, --compound    for compound-periphery"
    echo "  -i, --yield       for brlc-yield-streamer"
    echo "  -x, --pix         for brlc-pix-cashier"
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
    pixv4=1
    cppv2=1
fi

# Process arguments
if [[ "$#" -gt 0 ]]; then
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
    -c | --compound)
        compound=1
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
    -4 | --pixv4)
        pixv4=1
        shift
        ;;
    -2 | --cppv2)
        cppv2=1
        shift
        ;;
    *)
        echo "Unknown option: $1"
        print_help
        exit 1
        ;;
    esac
fi

# Execute
if [ "$token" == 1 ]; then
    test brlc-token BRLCToken "$@"
    test brlc-token base/CWToken.complex "$@"
    test brlc-token BRLCTokenBridgeable "$@"
    test brlc-token USJimToken "$@"
fi

if [ "$pix" == 1 ]; then
    # Cashier Transition: test Cashier v4, regardless if the repository was renamed or not
    test brlc-cashier CashierSharded "$@" || test brlc-pix-cashier CashierSharded "$@"
fi

if [ "$yield" == 1 ]; then
    test brlc-yield-streamer BalanceTracker "$@"
    test brlc-yield-streamer YieldStreamer "$@"
fi

if [ "$periphery" == 1 ]; then
    test brlc-periphery CardPaymentProcessor "$@"
    test brlc-periphery CashbackDistributor "$@"
fi

if [ "$multisig" == 1 ]; then
    test brlc-multisig MultiSigWallet "$@"
    test brlc-multisig MultiSigWalletFactory "$@"
    test brlc-multisig MultiSigWalletUpgradeable "$@"
fi

if [ "$compound" == 1 ]; then
    test compound-periphery CompoundAgent "$@"
fi

# Alternative versions

if [ "$cppv2" == 1 ]; then
    test brlc-periphery-v2 CardPaymentProcessor "$@"
fi
