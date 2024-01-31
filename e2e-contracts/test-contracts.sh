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
    cp ../../../e2e/hardhat.config.ts .
    rm -rf .openzeppelin/

    # test
    npx hardhat test --bail --network stratus test/$test.test.ts

    # go back to previous directory
    cd -
}

# ------------------------------------------------------------------------------
# Execution
# ------------------------------------------------------------------------------

# configure tools
asdf local nodejs 20.10.0

while getopts tpmch flag
do
    case "${flag}" in
        t) token=1;;
        p) periphery=1;;
        m) multisig=1;;
        c) compound=1;;
        h) echo "Usage: test-contracts.sh [-t] [-p] [-m] [-c]"; 
           echo "-t for brlc-token";
           echo "-p for brlc-periphery";
           echo "-m for brlc-multisig";
           echo "-c for compound-periphery";
           echo "No parameter execute all tests";
           exit;;
    esac
done

if [ "$#" == 0 ]; then
    token=1
    periphery=1
    multisig=1
    compound=1
fi

# execute
if [ "$token" == 1 ]; then
    test brlc-token BRLCToken 
    test brlc-token BRLCToken.base 
    test brlc-token BRLCToken.complex 
    test brlc-token BRLCTokenBridgeable 
    test brlc-token InfinitePointsToken 
    test brlc-token LightningBitcoin 
    test brlc-token USJimToken 
fi

if [ "$periphery" == 1 ]; then
    test brlc-periphery CardPaymentProcessor 
    test brlc-periphery CashbackDistributor 
    test brlc-periphery PixCashier 
    test brlc-periphery TokenDistributor 
fi

if [ "$multisig" == 1 ]; then
    test brlc-multisig MultiSigWallet 
    test brlc-multisig MultiSigWalletFactory 
    test brlc-multisig MultiSigWalletUpgradeable 
fi

if [ "$compound" == 1 ]; then
    test compound-periphery CompoundAgent 
fi