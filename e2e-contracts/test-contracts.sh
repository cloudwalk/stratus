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

# execute
test brlc-token BRLCToken.base
test brlc-token BRLCToken.complex
test brlc-token BRLCTokenBridgeable
test brlc-token InfinitePointsToken
test brlc-token LightningBitcoin
test brlc-token USJimToken

test brlc-periphery CardPaymentProcessor
test brlc-periphery CashbackDistributor
test brlc-periphery PixCashier
test brlc-periphery TokenDistributor

test brlc-multisig MultiSigWallet
test brlc-multisig MultiSigWalletFactory
test brlc-multisig MultiSigWalletUpgradeable

test compound-periphery CompoundAgent