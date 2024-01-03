# Project: Show available tasks
default:
    just --list --unsorted

# Project: Run project setup
setup:
    @echo "* Installing Cargo killport"
    cargo install killport

    @echo "* Installing Cargo wait-service"
    cargo install wait-service

    @echo "* Cloning Solidity repositories"
    just contracts-clone

# ------------------------------------------------------------------------------
# Ledger tasks
# ------------------------------------------------------------------------------

# Ledger: Run locally with debug options
run:
    RUST_LOG=ledger=info cargo run

# Ledger: Run locally with release options
run-release:
    RUST_LOG=info cargo run --release

# Ledger: Compile with debug options
build:
    cargo build

# Ledger: Compile with release options
build-release:
    cargo build --release

# Ledger: Clean build artifacts
clean:
    cargo clean

# Ledger: Build documentation
doc:
    @just test-doc
    cargo +nightly doc --no-deps

# Ledger: Lint and format code
lint:
    cargo +nightly fmt --all
    cargo +nightly clippy --all-targets

# Ledger: Compile SQLx queries
sqlx:
    SQLX_OFFLINE=true cargo sqlx prepare --database-url postgres://postgres:123@0.0.0.0:5432/ledger -- --all-targets

# ------------------------------------------------------------------------------
# Test tasks
# ------------------------------------------------------------------------------

# Test: Execute all Rust tests
test name="":
    @just test-doc {{name}}
    @just test-unit {{name}}
    @just test-int {{name}}

# Test: Execute Rust doc tests
test-doc name="":
    cargo +nightly test {{name}} --doc

# Test: Execute Rust unit tests
test-unit name="":
    cargo test --lib {{name}} -- --nocapture

# Test: Execute Rust integration tests
test-int name="":
    cargo test --test '*' {{name}} -- --nocapture



# ------------------------------------------------------------------------------
# E2E tasks
# ------------------------------------------------------------------------------

# E2E: Execute Hardhat tests in the specified network
e2e network="ledger":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi
    if [ ! -d node_modules ]; then
        npm install
    fi
    npx hardhat test test/*.test.ts --network {{network}}

# E2E: Starts and execute Hardhat tests in Anvil
e2e-anvil:
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi

    echo "-> Starting Anvil"
    anvil --chain-id 2008 --gas-price 0 --block-base-fee-per-gas 0 --port 8546 &

    echo "-> Waiting Anvil to start"
    wait-service --tcp localhost:8546 -- echo

    echo "-> Running E2E tests"
    just e2e anvil

    echo "-> Killing Anvil"
    killport 8546

# E2E: Starts and execute Hardhat tests in Hardhat
e2e-hardhat:
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi

    echo "-> Starting Hardhat"
    npx hardhat node &

    echo "-> Waiting Hardhat to start"
    wait-service --tcp localhost:8545 -- echo

    echo "-> Running E2E tests"
    just e2e hardhat

    echo "-> Killing Hardhat"
    killport 8545

# E2E: Starts and execute Hardhat tests in Ledger
e2e-ledger:
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi

    echo "-> Starting Ledger"
    RUST_LOG=info just run &

    echo "-> Waiting Ledger to start"
    wait-service --tcp localhost:3000 -- echo

    echo "-> Running E2E tests"
    just e2e ledger

    echo "-> Killing Ledger"
    killport 3000

# E2E: Lint and format code
e2e-lint:
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi
    node_modules/.bin/prettier . --write

# ------------------------------------------------------------------------------
# Contracts tasks
# ------------------------------------------------------------------------------

# Contracts: Clone Solidity repositories
contracts-clone:
    cd e2e-contracts && ./clone-contracts.sh

# Contracts: Compile selected Solidity contracts
contracts-compile:
    cd e2e-contracts && ./compile-contracts.sh

# Contracts: Test selected Solidity contracts on Ledger
contracts-test:
    cd e2e-contracts && ./test-contracts.sh
alias e2e-contracts := contracts-test
