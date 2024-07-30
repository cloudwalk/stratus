import '.justfile_helpers' # _lint, _outdated

# Environment variables automatically passed to executed commands.
export CARGO_PROFILE_RELEASE_DEBUG := env("CARGO_PROFILE_RELEASE_DEBUG", "1")
export RUST_BACKTRACE := env("RUST_BACKTRACE", "0")

# Global arguments that can be passed to receipts.
nightly_flag := if env("NIGHTLY", "") =~ "(true|1)" { "+nightly" } else { "" }
release_flag := if env("RELEASE", "") =~ "(true|1)" { "--release" } else { "" }
database_url := env("DATABASE_URL", "postgres://postgres:123@0.0.0.0:5432/stratus")
wait_service_timeout := env("WAIT_SERVICE_TIMEOUT", "60")

# Cargo flags.
build_flags := nightly_flag + " " + release_flag + " "

# Project: Show available tasks
default:
    just --list --unsorted

# Project: Run project setup
setup:
    @echo "* Installing Cargo killport"
    cargo install killport

    @echo "* Installing Cargo wait-service"
    cargo install wait-service

    @echo "* Installing Cargo flamegraph"
    cargo install flamegraph

    @echo "* Cloning Solidity repositories"
    just contracts-clone

# ------------------------------------------------------------------------------
# Stratus tasks
# ------------------------------------------------------------------------------

alias run := stratus

# Stratus: Compile with debug options
build:
    cargo build {{build_flags}}

# Stratus: Check, or compile without generating code
check:
    cargo check

# Stratus: Check all features individually using cargo hack
check-features *args="":
    command -v cargo-hack >/dev/null 2>&1 || { cargo install cargo-hack; }
    cargo hack check --each-feature --keep-going {{args}}

# Stratus: Clean build artifacts
clean:
    cargo clean

# Stratus: Build documentation
doc nightly-version="":
    @just _doc "{{nightly-version}}"

# Stratus: Lint and format code
lint:
    @just _lint

# Stratus: Lint and check code formatting
lint-check nightly-version="":
    @just _lint "{{nightly-version}}" --check "-D warnings"

# Stratus: Check for dependencies major updates
outdated:
    @just _outdated

# Stratus: Update only the project dependencies
update:
    cargo update stratus

# ------------------------------------------------------------------------------
# Database tasks
# ------------------------------------------------------------------------------

# Database: Compile SQLx queries
db-compile:
    SQLX_OFFLINE=true cargo sqlx prepare --database-url {{database_url}} -- --all-targets
alias sqlx := db-compile

# ------------------------------------------------------------------------------
# Additional binaries
# ------------------------------------------------------------------------------

# Bin: Stratus main service
stratus *args="":
    cargo run --bin stratus {{build_flags}} --features dev -- --enable-genesis {{args}}

# Bin: Download external RPC blocks and receipts to temporary storage
rpc-downloader *args="":
    cargo run --bin rpc-downloader {{build_flags}} -- {{args}}

# Bin: Import external RPC blocks from temporary storage to Stratus storage
importer-offline *args="":
    cargo run --bin importer-offline {{build_flags}} -- {{args}}

# Bin: Import external RPC blocks from external RPC endpoint to Stratus storage
importer-online *args="":
    cargo run --bin importer-online {{build_flags}} -- {{args}}

# Bin: Validate Stratus storage slots matches reference slots
state-validator *args="":
    cargo run --bin state-validator {{build_flags}} -- {{args}}

# Bin: `stratus` and `importer-online` in a single binary
run-with-importer *args="":
    cargo run --bin run-with-importer {{build_flags}} -- {{args}}

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
    cargo test {{name}} --doc

# Test: Execute Rust unit tests
test-unit name="":
    cargo test --lib {{name}} -- --nocapture --test-threads=1

# Test: Execute Rust integration tests
test-int name="'*'":
    cargo test --test {{name}} {{release_flag}} -- --nocapture

# ------------------------------------------------------------------------------
# E2E tasks
# ------------------------------------------------------------------------------

# E2E: Execute Hardhat tests in the specified network
e2e network="stratus" block-mode="automine" test="":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi
    if [ ! -d node_modules ]; then
        npm install
    fi

    if [ -z "{{test}}" ]; then
        BLOCK_MODE={{block-mode}} npx hardhat test test/{{block-mode}}/*.test.ts --network {{network}}
    else
        BLOCK_MODE={{block-mode}} npx hardhat test test/{{block-mode}}/*.test.ts --network {{network}} --grep "{{test}}"
    fi

# E2E: Starts and execute Hardhat tests in Hardhat
e2e-hardhat block-mode="automine" test="":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi

    echo "-> Starting Hardhat"
    BLOCK_MODE={{block-mode}} npx hardhat node &

    echo "-> Waiting Hardhat to start"
    wait-service --tcp localhost:8545 -- echo

    echo "-> Running E2E tests"
    just e2e hardhat {{block-mode}} {{test}}

    echo "-> Killing Hardhat"
    killport 8545

# E2E: Starts and execute Hardhat tests in Stratus
e2e-stratus block-mode="automine" test="":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi

    echo "-> Starting Stratus"
    just build || exit 1
    just run -a 0.0.0.0:3000 --block-mode {{block-mode}} > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{wait_service_timeout}} -- echo

    echo "-> Running E2E tests"
    just e2e stratus {{block-mode}} {{test}}
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000
    exit $result_code

# E2E: Starts and execute Hardhat tests in Stratus
e2e-stratus-rocks block-mode="automine" test="":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi

    echo "-> Starting Stratus"
    just build || exit 1
    just run -a 0.0.0.0:3000 --block-mode {{block-mode}} --perm-storage=rocks > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{wait_service_timeout}} -- echo

    echo "-> Running E2E tests"
    just e2e stratus {{block-mode}} {{test}}
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000
    exit $result_code

# E2E Clock: Builds and runs Stratus with block-time flag, then validates average block generation time
e2e-clock-stratus:
    #!/bin/bash
    echo "-> Starting Stratus"
    just build || exit 1
    cargo run  --release --bin stratus --features dev -- --block-mode 1s -a 0.0.0.0:3000 > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{wait_service_timeout}} -- echo

    echo "-> Validating block time"
    ./utils/block-time-check.sh
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000
    exit $result_code

# E2E Clock: Builds and runs Stratus Rocks with block-time flag, then validates average block generation time
e2e-clock-stratus-rocks:
    #!/bin/bash
    echo "-> Starting Stratus"
    just build || exit 1
    cargo run  --release --bin stratus --features dev -- --block-mode 1s --perm-storage=rocks -a 0.0.0.0:3000 > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{wait_service_timeout}} -- echo

    echo "-> Validating block time"
    ./utils/block-time-check.sh
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000
    exit $result_code

# E2E: Lint and format code
e2e-lint mode="--write":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi
    node_modules/.bin/prettier . {{ mode }} --ignore-unknown

# E2E: profiles RPC sync and generates a flamegraph
e2e-flamegraph:
    #!/bin/bash

    # Start PostgreSQL
    echo "Starting PostgreSQL"
    docker compose down -v
    docker compose up -d --force-recreate

    # Wait for PostgreSQL
    echo "Waiting for PostgreSQL to be ready"
    wait-service --tcp 0.0.0.0:5432 -t {{wait_service_timeout}} -- echo
    sleep 1

    # Start RPC mock server
    echo "Starting RPC mock server"
    killport 3003

    cd e2e/rpc-mock-server
    if [ ! -d node_modules ]; then
        npm install
    fi
    cd ../..
    node ./e2e/rpc-mock-server/index.js &
    sleep 1

    # Wait for RPC mock server
    echo "Waiting for RPC mock server to be ready..."
    wait-service --tcp 0.0.0.0:3003 -t {{wait_service_timeout}} -- echo

    # Run cargo flamegraph with necessary environment variables
    echo "Running cargo flamegraph"
    cargo flamegraph --bin importer-online --deterministic --features dev -- --external-rpc=http://localhost:3003/rpc --chain-id=2009

e2e-importer-online:
    #!/bin/bash

    just e2e-importer-online-up
    result_code=$?

    just e2e-importer-online-down

    exit $result_code

# E2E: Importer Online
e2e-importer-online-up:
    #!/bin/bash

    # Build Stratus and Run With Importer binaries
    echo "Building Stratus and Run With Importer binaries"
    cargo build --release --bin stratus --bin run-with-importer --features dev

    mkdir e2e_logs

    # Start Stratus binary
    RUST_LOG=info cargo run --release --bin stratus --features dev -- --block-mode 1s --enable-genesis --perm-storage=rocks --rocks-path-prefix=temp_3000 --tokio-console-address=0.0.0.0:6668 --metrics-exporter-address=0.0.0.0:9000 -a 0.0.0.0:3000 > e2e_logs/stratus.log &

    # Wait for Stratus to start
    wait-service --tcp 0.0.0.0:3000 -t {{wait_service_timeout}} -- echo

    # Start Run With Importer binary
    RUST_LOG=info cargo run --release --bin run-with-importer --features dev -- --block-mode 1s --perm-storage=rocks --rocks-path-prefix=temp_3001 --tokio-console-address=0.0.0.0:6669 --metrics-exporter-address=0.0.0.0:9001 -a 0.0.0.0:3001 -r http://0.0.0.0:3000/ -w ws://0.0.0.0:3000/ > e2e_logs/run_with_importer.log &

    # Wait for Run With Importer to start
    wait-service --tcp 0.0.0.0:3001 -t {{wait_service_timeout}} -- echo

    if [ -d e2e/cloudwalk-contracts ]; then
    (
        cd e2e/cloudwalk-contracts/integration
        npm install
        npx hardhat test test/importer.test.ts --network stratus --bail
        if [ $? -ne 0 ]; then
            echo "Tests failed"
            exit 1
        else
            echo "Tests passed successfully"
            exit 0
        fi
    )
    fi

# E2E: Importer Online
e2e-importer-online-down:
    #!/bin/bash

    # Kill run-with-importer
    killport 3001
    run_with_importer_pid=$(pgrep -f 'run-with-importer')
    kill $run_with_importer_pid

    # Kill Stratus
    killport 3000
    stratus_pid=$(pgrep -f 'stratus')
    kill $stratus_pid

    # Delete data contents
    rm -rf ./temp_*

    # Delete zeppelin directory
    rm -rf ./e2e/cloudwalk-contracts/integration/.openzeppelin

# ------------------------------------------------------------------------------
# Chaos tests
# ------------------------------------------------------------------------------

# Chaos: Run chaos testing experiment
run-chaos-experiment bin="" instances="" iterations="" enable-leader-restart="" experiment="":
    #!/bin/bash

    echo "Building Stratus"
    cargo build --release --bin {{ bin }} --features dev

    cd e2e/cloudwalk-contracts/integration
    if [ ! -d node_modules ]; then
        npm install
    fi
    cd ../../..

    echo "Executing experiment {{ experiment }} {{ iterations }}x on {{ bin }} binary with {{ instances }} instance(s)"
    ./chaos/experiments/{{ experiment }}.sh --bin {{ bin }} --instances {{ instances }} --iterations {{ iterations }} --enable-leader-restart {{ enable-leader-restart }}

# ------------------------------------------------------------------------------
# Hive tests
# ------------------------------------------------------------------------------

# Hive: Build Stratus image for hive task
hive-build-client:
    docker build -f hive/clients/stratus/Dockerfile_base -t stratus_base .

# Hive: Execute test pipeline
hive:
    if ! docker images | grep -q stratus_base; then \
        echo "Building Docker image..."; \
        docker build -f hive/clients/stratus/Dockerfile_base -t stratus_base .; \
    else \
        echo "Docker image already built."; \
    fi
    cd hive && go build .
    cd hive && ./hive --client stratus --sim stratus/rpc --sim.parallelism 10
#    cd hive && sudo ./hive --client stratus --sim stratus/rpc --sim.parallelism 10 --loglevel 5 --docker.output

# Hive: View test pipeline results in Hiveview
hiveview:
    cd hive && go build ./cmd/hiveview
    ./hive/hiveview --serve --addr 0.0.0.0:8080 --logdir ./hive/workspace/logs/


# ------------------------------------------------------------------------------
# Contracts tasks
# ------------------------------------------------------------------------------

# Contracts: Clone Solidity repositories
contracts-clone *args="":
    cd e2e/cloudwalk-contracts && ./contracts-clone.sh {{args}}

# Contracts: Compile selected Solidity contracts
contracts-compile:
    cd e2e/cloudwalk-contracts && ./contracts-compile.sh

# Contracts: Flatten solidity contracts for integration test
contracts-flatten *args="":
    cd e2e/cloudwalk-contracts && ./contracts-flatten.sh {{args}}

# Contracts: Test selected Solidity contracts on Stratus
contracts-test *args="":
    cd e2e/cloudwalk-contracts && ./contracts-test.sh {{args}}
alias e2e-contracts := contracts-test

# Contracts: Remove all the cloned repositories
contracts-remove *args="":
    cd e2e/cloudwalk-contracts && ./contracts-remove.sh {{args}}

# Contracts: Start Stratus and run contracts tests with InMemory storage
contracts-test-stratus *args="":
    #!/bin/bash
    echo "-> Starting Stratus"
    just build || exit 1
    just run -a 0.0.0.0:3000 > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{wait_service_timeout}} -- echo

    echo "-> Running E2E Contracts tests"
    just e2e-contracts {{args}}
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000
    exit $result_code

# Contracts: Start Stratus and run contracts tests with RocksDB storage
contracts-test-stratus-rocks *args="":
    #!/bin/bash
    echo "-> Starting Stratus"
    just build || exit 1
    just run -a 0.0.0.0:3000 --perm-storage=rocks > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{wait_service_timeout}} -- echo

    echo "-> Running E2E tests"
    just e2e-contracts {{args}}
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000

    exit $result_code

# Contracts: Run tests and generate coverage info. Use --html to open in browser.
contracts-coverage *args="":
    cd e2e/cloudwalk-contracts && ./contracts-coverage.sh {{args}}

# Contracts: Erase coverage info
contracts-coverage-erase:
    #!/bin/bash
    cd e2e/cloudwalk-contracts/repos || exit 1
    echo "Erasing coverage info..."
    rm -rf ./*/coverage && echo "Coverage info erased."

