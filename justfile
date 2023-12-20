# Runs the service locally
run:
    cargo run

# Runs the service locally with release options
run-release:
    cargo run --release

# Compile project with debug options
build:
    cargo build

# Compile project with release options
build-release:
    cargo build --release

# Clean project build directory
clean:
    cargo clean

# Compile SQLx queries
sqlx:
    SQLX_OFFLINE=true cargo sqlx prepare --database-url postgres://postgres:123@0.0.0.0:5432/ledger -- --all-targets

# Execute all tests
test name="":
    @just test-doc {{name}}
    @just test-unit {{name}}
    @just test-int {{name}}

# Execute doc tests
test-doc name="":
    cargo +nightly test {{name}} --doc

# Execute unit tests
test-unit name="":
    cargo test --lib {{name}} -- --nocapture

# Execute integration tests
test-int name="":
    cargo test --test '*' {{name}} -- --nocapture

# Execute E2E tests
e2e network="ledger":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi
    npx hardhat test test/*.test.ts --network {{network}}

# Lint E2E tests
e2e-lint:
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi
    node_modules/.bin/prettier . --write

# Execute E2E tests with Anvil
e2e-anvil:
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi

    echo "-> Starting Anvil"
    anvil --chain-id 2008 --gas-price 0 --block-base-fee-per-gas 0 --port 8546 &

    echo "-> Waiting Anvil to start"
    sleep 0.1

    echo "-> Running E2E tests"
    just e2e anvil

    echo "-> Killing Anvil"
    lsof -n -i :8546 | grep -v PID | awk '{print $2}' | xargs -I{} kill -9 {}

# Execute E2E tests with Hardhat
e2e-hardhat:
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi

    echo "-> Starting Hardhat"
    npx hardhat node &

    echo "-> Waiting Hardhat to start"
    sleep 1

    echo "-> Running E2E tests"
    just e2e hardhat

    echo "-> Killing Hardhat"
    lsof -n -i :8545 | grep -v PID | awk '{print $2}' | xargs -I{} kill -9 {}

# Execute E2E tests with Ledger
e2e-ledger:
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi

    echo "-> Starting Ledger"
    just run &

    echo "-> Waiting Ledger to start"
    sleep 1

    echo "-> Running E2E tests"
    just e2e ledger

    echo "-> Killing Ledger"
    lsof -n -i :3000 | grep -v PID | awk '{print $2}' | xargs -I{} kill -9 {}

# Generate documentation
doc:
    @just test-doc
    cargo +nightly doc --no-deps

# Format code and run configured linters
lint:
    cargo +nightly fmt --all
    cargo +nightly clippy --all-targets

