#!/bin/bash
#
# Runs tests for Solidity contracts.
#
source $(dirname $0)/_functions.sh

# ------------------------------------------------------------------------------
# Functions
# ------------------------------------------------------------------------------
test() {
    repo=$1
    test=$2
    log "Testing: $test ($repo)"

    # configure hardhat env
    cd repos/$repo
    git restore .
    git apply ../../patches/$repo.patch || true
    cp ../../../e2e/hardhat.config.ts .
    rm -rf .openzeppelin/

    # test
    npx hardhat test --bail --network stratus test/$test.test.ts
    result_code=$?

    # restore original files
    git restore .

    # go back to previous directory
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
asdf local nodejs 20.10.0

while getopts tpmchix flag
do
    case "${flag}" in
        t) token=1;;
        p) periphery=1;;
        m) multisig=1;;
        c) compound=1;;
        i) yield=1;;
        x) pix=1;;
        h) echo "Usage: test-contracts.sh [-[tpchix]]";
           echo "-t for brlc-token";
           echo "-p for brlc-periphery";
           echo "-m for brlc-multisig";
           echo "-c for compound-periphery";
           echo "-i for brlc-yield-streamer";
           echo "-x for brlc-pix-cashier";
           echo "No parameter execute all tests";
           exit;;
    esac
done

if [ "$#" == 0 ]; then
    token=1
    periphery=1
    multisig=1
    compound=1
    yield=1
    pix=1
fi

# execute
if [ "$token" == 1 ]; then
    test brlc-token BRLCToken
    test brlc-token base/CWToken.complex
    test brlc-token BRLCTokenBridgeable
    test brlc-token USJimToken
fi

if [ "$pix" == 1 ]; then
    test brlc-pix-cashier PixCashier
fi

if [ "$yield" == 1 ]; then
    test brlc-yield-streamer BalanceTracker
    test brlc-yield-streamer YieldStreamer
fi

if [ "$periphery" == 1 ]; then
    test brlc-periphery CardPaymentProcessor
    test brlc-periphery CashbackDistributor
fi

if [ "$multisig" == 1 ]; then
    test brlc-multisig MultiSigWallet
    test brlc-multisig MultiSigWalletFactory
    test brlc-multisig MultiSigWalletUpgradeable
fi

if [ "$compound" == 1 ]; then
    test compound-periphery CompoundAgent
fi
