import "justfile_helpers"

# Environment variables automatically passed to executed commands.
export CARGO_PROFILE_RELEASE_DEBUG := env("CARGO_PROFILE_RELEASE_DEBUG", "1")
export RUST_BACKTRACE := env("RUST_BACKTRACE", "0")
export CARGO_COMMAND := env("CARGO_COMMAND", "")

# Global arguments that can be passed to receipts.
nightly_flag := if env("NIGHTLY", "") =~ "(true|1)" { "+nightly" } else { "" }
release_flag := if env("RELEASE", "") =~ "(true|1)" { "--release" } else { "" }
database_url := env("DATABASE_URL", "postgres://postgres:123@0.0.0.0:5432/stratus")

# Project: Show available tasks
default:
    just --list --unsorted

# Project: Run project setup
setup:
    @just _log "Installing Cargo killport"
    cargo install killport

    @just _log "Installing Cargo wait-service"
    cargo install wait-service

    @just _log "Installing Cargo flamegraph"
    cargo install flamegraph

    @just _log "Cloning Solidity repositories"
    just contracts-clone

# ------------------------------------------------------------------------------
# Stratus tasks
# ------------------------------------------------------------------------------

alias run          := stratus
alias run-leader   := stratus
alias run-follower := stratus-follower
alias run-importer := stratus-follower

# Stratus: Compile with debug options
build binary="stratus" features="dev":
    cargo {{nightly_flag}} build {{release_flag}} --bin {{binary}} --features {{features}}

# Stratus: Check, or compile without generating code
check:
    cargo {{nightly_flag}} check

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
lint-check nightly-version="" clippy-flags="-D warnings -A clippy::unwrap_used -A clippy::expect_used -A clippy::panic":
    @just _lint "{{nightly-version}}" --check "{{ clippy-flags }}"

# Stratus: Check for dependencies major updates
outdated:
    #!/bin/bash
    command -v cargo-outdated >/dev/null 2>&1 || { cargo install cargo-outdated; }
    cargo outdated --root-deps-only --ignore-external-rel

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

# Bin: Stratus main service as leader
stratus *args="":
    cargo {{nightly_flag}} run --bin stratus {{release_flag}} --features dev -- --leader {{args}}

# Bin: Stratus main service as leader while performing memory-profiling, producing a heap dump every 2^32 allocated bytes (~4gb)
# To produce a flamegraph of the memory usage use jeprof:
#   * Diferential flamegraph: jeprof <binary> --base=./jeprof.<...>.i0.heap ./jeprof.<...>.i<n>.heap --collapsed | flamegraph.pl > mem_prof.svg
#   * Point in time flamegraph: jeprof <binary> ./jeprof.<...>.i<n>.heap --collapsed | flamegraph.pl > mem_prof.svg
stratus-memory-profiling *args="":
    _RJEM_MALLOC_CONF=prof:true,prof_final:true,prof_leak:true,prof_gdump:true,lg_prof_interval:32 cargo {{nightly_flag}} run --bin stratus {{release_flag}} --features dev,jeprof -- --leader {{args}}

# Bin: Stratus main service as follower
stratus-follower *args="":
    LOCAL_ENV_PATH=config/stratus-follower.env.local cargo {{nightly_flag}} run --bin stratus {{release_flag}} --features dev -- --follower {{args}}

# Bin: Download external RPC blocks and receipts to temporary storage
rpc-downloader *args="":
    cargo {{nightly_flag}} run --bin rpc-downloader {{release_flag}} -- {{args}}

# Bin: Import external RPC blocks from temporary storage to Stratus storage
importer-offline *args="":
    cargo {{nightly_flag}} run --bin importer-offline {{release_flag}} -- {{args}}

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

# Test: Execute coverage for a specific group
stratus-test-coverage-group group="unit" *args="":
    #!/bin/bash

    cargo llvm-cov clean --workspace
    
    rm -rf temp_*
    rm -rf data/rocksdb
    rm -rf data/importer-offline-database-rocksdb
    rm -rf e2e_logs
    
    source <(cargo llvm-cov show-env --export-prefix)
    export RUST_LOG=error
    export TRACING_LOG_FORMAT=json
    
    case "{{group}}" in
        "unit")
            cargo llvm-cov --no-report
            ;;
        "inmemory")
            just contracts-clone
            just contracts-flatten
            
            for test in "automine" "external"; do
                just _coverage-run-stratus-recipe e2e-stratus $test
                rm -rf e2e_logs
                rm -rf temp_*
            done
            
            just _coverage-run-stratus-recipe e2e-clock-stratus
            rm -rf e2e_logs
            rm -rf temp_*
            
            just _coverage-run-stratus-recipe contracts-test-stratus
            rm -rf e2e_logs
            rm -rf temp_*
            
            just _coverage-run-stratus-recipe e2e-eof
            rm -rf e2e_logs
            rm -rf temp_*
            ;;
        "rocksdb")
            just contracts-clone
            just contracts-flatten
            
            for test in "automine" "external"; do
                rm -rf data/rocksdb
                just _coverage-run-stratus-recipe e2e-stratus-rocks $test
                rm -rf e2e_logs
                rm -rf temp_*
                rm -rf data/rocksdb
            done
            
            rm -rf data/rocksdb
            just _coverage-run-stratus-recipe e2e-clock-stratus-rocks
            rm -rf e2e_logs
            rm -rf temp_*
            rm -rf data/rocksdb
            
            rm -rf data/rocksdb
            just _coverage-run-stratus-recipe contracts-test-stratus-rocks
            rm -rf e2e_logs
            rm -rf temp_*
            rm -rf data/rocksdb

            rm -rf data/rocksdb
            just _coverage-run-stratus-recipe e2e-eof rocks
            rm -rf e2e_logs
            rm -rf temp_*
            rm -rf data/rocksdb
            ;;
        "leader-follower")
            just contracts-clone
            just contracts-flatten
            
            for test in kafka deploy brlc change miner importer health; do
                just _e2e-leader-follower-up-coverage $test
                rm -rf e2e_logs
                rm -rf temp_*
                rm -rf utils/deploy/deploy_*.log
            done
            ;;
        "admin-password")
            just _coverage-run-stratus-recipe e2e-admin-password
            rm -rf e2e_logs
            rm -rf temp_*
            ;;
        "rpc-downloader")
            just _coverage-run-stratus-recipe e2e-rpc-downloader
            rm -rf e2e_logs
            rm -rf temp_*
            ;;
        "importer-offline")
            just _coverage-run-stratus-recipe e2e-importer-offline
            rm -rf e2e_logs
            rm -rf temp_*
            rm -rf data/importer-offline-database-rocksdb
            ;;
        *)
            echo "Unknown group: {{group}}"
            exit 1
            ;;
    esac
    
    # Ensure the output directory exists
    mkdir -p target/llvm-cov/codecov
    
    # Generate the report with the specified arguments
    # If --output-path is provided in args, use it, otherwise use the default path
    if [[ "{{args}}" == *"--output-path"* ]]; then
        cargo llvm-cov report {{args}}
    else
        # Default output path for codecov
        cargo llvm-cov report --codecov --output-path target/llvm-cov/codecov/{{group}}.json {{args}}
    fi

# ------------------------------------------------------------------------------
# E2E tasks
# ------------------------------------------------------------------------------

# E2E: Execute Hardhat tests in the specified network
e2e network="stratus" block_modes="automine" test="":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi
    if [ ! -d node_modules ]; then
        npm install
    fi

    block_modes_split=$(echo {{block_modes}} | sed "s/,/ /g")
    for block_mode in $block_modes_split
    do
        just _log "Executing: $block_mode"
        if [ -z "{{test}}" ]; then
            BLOCK_MODE=$block_mode npx hardhat test test/$block_mode/*.test.ts --network {{network}}
        else
            BLOCK_MODE=$block_mode npx hardhat test test/$block_mode/*.test.ts --network {{network}} --grep "{{test}}"
        fi
    done

# E2E: Execute admin password tests
e2e-admin-password:
    #!/bin/bash

    mkdir -p e2e_logs
    cd e2e
    
    npm install

    for test in "enabled|test123" "disabled|"; do
        IFS="|" read -r type pass <<< "$test"
        just _log "Running admin password tests with password $type"
        ADMIN_PASSWORD=$pass just run -a 0.0.0.0:3000 > /dev/null &
        just _wait_for_stratus
        
        npx hardhat test test/admin/e2e-admin-password-$type.test.ts --network stratus
        
        killport 3000 -s sigterm
    done

# E2E: Execute EOF (EVM Object Format) tests
e2e-eof perm-storage="inmemory":
    #!/bin/bash
    cd e2e/eof

    forge install

    # Start Stratus
    just build
    just run -a 0.0.0.0:3000 --executor-evm-spec Osaka --perm-storage={{perm-storage}} > stratus.log &
    just _wait_for_stratus

    # Run tests using alice pk
    forge script test/TestEof.s.sol:TestEof --rpc-url http://0.0.0.0:3000/ --broadcast -vvvv --legacy --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 --sig "deploy()" --slow
    forge script test/TestEof.s.sol:TestEof --rpc-url http://0.0.0.0:3000/ --broadcast -vvvv --legacy --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 --sig "run()" --slow

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
    just build

    just _log "Starting Stratus"
    RUST_LOG=debug just run -a 0.0.0.0:3000 --block-mode {{block-mode}} > stratus.log &

    just _wait_for_stratus

    just _log "Running E2E tests"
    if [[ {{block-mode}} =~ ^[0-9]+(ms|s)$ ]]; then
        just e2e stratus interval "{{test}}"
    else
        just e2e stratus {{block-mode}} "{{test}}"
    fi
    result_code=$?

    just _log "Killing Stratus"
    killport 3000 -s sigterm
    exit $result_code

# E2E: Starts and execute Hardhat tests in Stratus
e2e-stratus-rocks block-mode="automine" test="":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi
    just build

    just _log "Starting Stratus"
    just run -a 0.0.0.0:3000 --block-mode {{block-mode}} --perm-storage=rocks > stratus.log &

    just _wait_for_stratus

    just _log "Running E2E tests"
    if [[ {{block-mode}} =~ ^[0-9]+(ms|s)$ ]]; then
        just e2e stratus interval "{{test}}"
    else
        just e2e stratus {{block-mode}} "{{test}}"
    fi
    result_code=$?

    just _log "Killing Stratus"
    killport 3000 -s sigterm
    exit $result_code

# E2E Clock: Builds and runs Stratus with block-time flag, then validates average block generation time
e2e-clock-stratus:
    #!/bin/bash
    just build
    just _log "Starting Stratus"
    just run --block-mode 1s -a 0.0.0.0:3000 > stratus.log &

    just _wait_for_stratus

    just _log "Validating block time"
    ./utils/block-time-check.sh
    result_code=$?

    just _log "Killing Stratus"
    killport 3000 -s sigterm
    exit $result_code

# E2E Clock: Builds and runs Stratus Rocks with block-time flag, then validates average block generation time
e2e-clock-stratus-rocks:
    #!/bin/bash
    just build

    just _log "Starting Stratus"
    just run --block-mode 1s --perm-storage=rocks -a 0.0.0.0:3000 > stratus.log &

    just _wait_for_stratus

    just _log "Validating block time"
    ./utils/block-time-check.sh
    result_code=$?

    just _log "Killing Stratus"
    killport 3000 -s sigterm
    exit $result_code

# E2E: Lint and format code
e2e-lint mode="--write":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi
    node_modules/.bin/prettier . {{ mode }} --ignore-unknown

# E2E: Lint and format shell scripts for cloudwalk contracts
shell-lint mode="--write":
    @command -v shfmt > /dev/null 2>&1 && command -v shellcheck > /dev/null 2>&1 || echo "Please, install shfmt and shellcheck" && exit 0
    @shfmt {{ mode }} --indent 4 e2e/cloudwalk-contracts/*.sh
    @shellcheck e2e/cloudwalk-contracts/*.sh --severity=warning --shell=bash

# E2E: profiles RPC sync and generates a flamegraph
e2e-flamegraph:
    #!/bin/bash

    # Start RPC mock server
    just _log "Starting RPC mock server"
    killport 3003 -s sigterm

    cd e2e/rpc-mock-server
    if [ ! -d node_modules ]; then
        npm install
    fi
    cd ../..
    node ./e2e/rpc-mock-server/index.js &
    sleep 1

    # Wait for RPC mock server
    just _log "Waiting for RPC mock server to start for 60 seconds"
    wait-service --tcp 0.0.0.0:3003 -t 60 -- echo "RPC mock server started"

    # Run cargo flamegraph with necessary environment variables
    just _log "Running cargo flamegraph"
    cargo flamegraph --bin importer-online --deterministic --features dev -- --external-rpc=http://localhost:3003/rpc --chain-id=2009

e2e-leader:
    #!/bin/bash
    RUST_BACKTRACE=1 RUST_LOG=info cargo ${CARGO_COMMAND} run {{release_flag}} --bin stratus --features dev -- --leader --block-mode 1s --perm-storage=rocks --rocks-path-prefix=temp_3000 -a 0.0.0.0:3000 > e2e_logs/stratus.log &
    just _wait_for_stratus 3000

e2e-follower test="brlc":
    #!/bin/bash
    if [ "{{test}}" = "kafka" ]; then
    # Start Kafka using Docker Compose
        just _log "Starting Kafka"
        docker-compose up kafka >> e2e_logs/kafka.log &
        just _log "Waiting Kafka start"
        wait-service --tcp 0.0.0.0:29092 -- echo
        docker exec kafka kafka-topics --create --topic stratus-events --bootstrap-server localhost:29092 --partitions 1 --replication-factor 1
        RUST_BACKTRACE=1 RUST_LOG=info cargo ${CARGO_COMMAND} run {{release_flag}} --bin stratus --features dev -- --follower --perm-storage=rocks --rocks-path-prefix=temp_3001 -a 0.0.0.0:3001 -r http://0.0.0.0:3000/ -w ws://0.0.0.0:3000/ --kafka-bootstrap-servers localhost:29092 --kafka-topic stratus-events --kafka-client-id stratus-producer --kafka-security-protocol none > e2e_logs/importer.log &
    else
        RUST_BACKTRACE=1 RUST_LOG=info cargo ${CARGO_COMMAND} run {{release_flag}} --bin stratus --features dev -- --follower --perm-storage=rocks --rocks-path-prefix=temp_3001 -a 0.0.0.0:3001 -r http://0.0.0.0:3000/ -w ws://0.0.0.0:3000/ > e2e_logs/importer.log &
    fi
    # Wait for Stratus with follower flag to start
    just _wait_for_stratus 3001


_e2e-leader-follower-up-impl test="brlc" release_flag="--release":
    #!/bin/bash
    just build

    mkdir e2e_logs

    # Start Stratus with leader flag
    just e2e-leader

    # Start Stratus with follower flag
    just e2e-follower {{test}}

    if [ "{{test}}" = "deploy" ]; then
        just _log "Running deploy script"
        cd utils/deploy
        poetry install --no-root

        for i in {1..5}; do
            poetry run python3 ./deploy.py --current-leader 0.0.0.0:3000 --current-follower 0.0.0.0:3001 --auto-approve --log-file deploy_01.log
            if [ $? -ne 0 ]; then
                just _log "Deploy script failed"
                exit 1
            fi

            just _log "Switching back roles..."
            sleep 5

            poetry run python3 ./deploy.py --current-leader 0.0.0.0:3001 --current-follower 0.0.0.0:3000 --auto-approve --log-file deploy_02.log
            if [ $? -ne 0 ]; then
                just _log "Deploy script failed"
                exit 1
            fi

            sleep 5
        done

        just _log "Deploy script ran successfully"
        exit 0
    elif [ -d e2e/cloudwalk-contracts ]; then
    (
        cd e2e/cloudwalk-contracts/integration
        npm install
        npx hardhat test test/leader-follower-{{test}}.test.ts --bail --network stratus --show-stack-traces
        if [ $? -ne 0 ]; then
            just _log "Tests failed"
            exit 1
        else
            just _log "Tests passed successfully"
            exit 0
        fi
    )
    fi

# E2E: Leader & Follower Up
e2e-leader-follower-up test="brlc" release_flag="--release":
    just _e2e-leader-follower-up-impl {{test}} {{release_flag}}
    killport 3000 -s sigterm
    killport 3001 -s sigterm

# E2E: Leader & Follower Down
e2e-leader-follower-down:
    #!/bin/bash

    # Kill Stratus
    killport 3001 -s sigterm
    killport 3000 -s sigterm
    stratus_pid=$(pgrep -f 'stratus')
    kill $stratus_pid

    # Kill Kafka
    docker-compose down

    # Delete data contents
    rm -rf ./temp_*

    # Delete zeppelin directory
    rm -rf ./e2e/cloudwalk-contracts/integration/.openzeppelin

# E2E: RPC Downloader test
e2e-rpc-downloader:
    #!/bin/bash
    mkdir e2e_logs

    just build

    just _log "Starting Stratus"
    just run -a 0.0.0.0:3000 > e2e_logs/e2e-rpc-downloader-stratus.log &
    just _wait_for_stratus

    just _log "Starting PostgreSQL"
    docker-compose up -d postgres

    just _log "Running TestContractBalances tests"
    just e2e stratus automine
    
    just _log "Running RPC Downloader test"
    just rpc-downloader --external-rpc http://localhost:3000/ --external-rpc-storage postgres://postgres:123@localhost:5432/stratus --metrics-exporter-address 0.0.0.0:9001

    just rpc-downloader --external-rpc http://localhost:3000/ --external-rpc-storage postgres://postgres:123@localhost:5432/stratus --metrics-exporter-address 0.0.0.0:9001

    just _log "Checking content of postgres"
    pip install -r utils/check_rpc_downloader/requirements.txt
    POSTGRES_DB=stratus POSTGRES_PASSWORD=123 ETH_RPC_URL=http://localhost:3000/ python utils/check_rpc_downloader/main.py --start 0

    just _log "Killing Stratus"
    killport 3000 -s sigterm

    just _log "Killing PostgreSQL"
    docker-compose down postgres

# E2E Importer Offline
e2e-importer-offline:
    mkdir -p e2e_logs

    rm -rf data/importer-offline-database-rocksdb

    just build

    just _log "Starting Stratus"
    just stratus -a 0.0.0.0:3000 > e2e_logs/e2e-importer-offline-stratus.log &
    just _wait_for_stratus

    just _log "Running TestContractBalances tests"
    just e2e stratus automine

    just _log "Starting PostgreSQL"
    docker-compose up -d postgres
    
    just _log "Running rpc downloader"
    just rpc-downloader --external-rpc http://localhost:3000/ --external-rpc-storage postgres://postgres:123@localhost:5432/stratus --metrics-exporter-address 0.0.0.0:9001 --initial-accounts 0x70997970c51812dc3a010c7d01b50e0d17dc79c8,0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266

    just _log "Run importer-offline"
    just importer-offline --external-rpc-storage postgres://postgres:123@localhost:5432/stratus --rocks-path-prefix=data/importer-offline-database --metrics-exporter-address 0.0.0.0:9002

    just _log "Stratus for importer-offline"
    cargo run --release --bin stratus -- --leader -a 0.0.0.0:3001 --perm-storage=rocks --rocks-path-prefix=data/importer-offline-database --metrics-exporter-address 0.0.0.0:9002 > e2e_logs/e2e-importer-offline-stratus-3001.log &
    just _wait_for_stratus 3001

    just _log "Compare blocks of stratus and importer-offline"
    pip install -r utils/compare_block/requirements.txt
    python utils/compare_block/main.py http://localhost:3000 http://localhost:3001 1 --ignore timestamp
    
    just _log "Killing Stratus"
    killport 3000 -s sigterm

    just _log "Killing importer-offline"
    killport 3001 -s sigterm
    
    just _log "Killing PostgreSQL"
    docker-compose down postgres

# ------------------------------------------------------------------------------
# Hive tests
# ------------------------------------------------------------------------------

# Hive: Build Stratus image for hive task
hive-build-client:
    docker build -f hive/clients/stratus/Dockerfile_base -t stratus_base .

# Hive: Execute test pipeline
hive:
    if ! docker images | grep -q stratus_base; then \
        just _log "Building Docker image..."; \
        docker build -f hive/clients/stratus/Dockerfile_base -t stratus_base .; \
    else \
        just _log "Docker image already built."; \
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
    just build

    just _log "Starting Stratus"
    just run -a 0.0.0.0:3000 &

    just _wait_for_stratus

    just _log "Running E2E Contracts tests"
    just e2e-contracts {{args}}
    result_code=$?

    just _log "Killing Stratus"
    killport 3000 -s sigterm
    exit $result_code

# Contracts: Start Stratus and run contracts tests with RocksDB storage
contracts-test-stratus-rocks *args="":
    #!/bin/bash
    just build

    just _log "Starting Stratus"
    just run -a 0.0.0.0:3000 --perm-storage=rocks > stratus.log &

    just _wait_for_stratus

    just _log "Running E2E tests"
    just e2e-contracts {{args}}
    result_code=$?

    just _log "Killing Stratus"
    killport 3000 -s sigterm

    exit $result_code

# Contracts: Run tests and generate coverage info. Use --html to open in browser.
contracts-coverage *args="":
    cd e2e/cloudwalk-contracts && ./contracts-coverage.sh {{args}}

# Contracts: Erase coverage info
contracts-coverage-erase:
    #!/bin/bash
    cd e2e/cloudwalk-contracts/repos || exit 1
    just _log "Erasing coverage info..."
    rm -rf ./*/coverage && echo "Coverage info erased."


_e2e-leader-follower-up-coverage test="":
    -rm -r temp_3000-rocksdb
    -rm -r temp_3001-rocksdb
    just _coverage-run-stratus-recipe e2e-leader-follower-up {{test}} " "
    sleep 10
    -rm -r e2e_logs
    -rm utils/deploy/deploy_01.log
    -rm utils/deploy/deploy_02.log

_coverage-run-stratus-recipe *recipe="":
    #!/bin/bash
    # Create logs directory if it doesn't exist
    mkdir -p e2e_logs
    
    # Run the recipe and capture the exit code
    just {{recipe}}
    result=$?
    
    # Wait for processes to finish
    sleep 10
    
    # Return the original exit code
    exit $result

stratus-test-coverage *args="":
    #!/bin/bash

    for group in unit inmemory rocksdb leader-follower admin-password rpc-downloader importer-offline; do
        just stratus-test-coverage-group $group {{args}}
    done
