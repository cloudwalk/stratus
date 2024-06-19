import '.justfile_helpers' # _lint, _outdated

# Environment variables automatically passed to executed commands.
export CARGO_PROFILE_RELEASE_DEBUG := env("CARGO_PROFILE_RELEASE_DEBUG", "1")
export RUST_BACKTRACE := env("RUST_BACKTRACE", "0")

# Global arguments that can be passed to receipts.
feature_flags := "dev," + env("FEATURES", "")
nightly_flag := if env("NIGHTLY", "") =~ "(true|1)" { "+nightly" } else { "" }
release_flag := if env("RELEASE", "") =~ "(true|1)" { "--release" } else { "" }
database_url := env("DATABASE_URL", "postgres://postgres:123@0.0.0.0:5432/stratus")
wait_service_timeout := env("WAIT_SERVICE_TIMEOUT", "60")

# Cargo flags.
build_flags := nightly_flag + " " + release_flag + " --bin stratus --features " + feature_flags
run_flags := "--enable-genesis --enable-test-accounts"

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

# Stratus: Run main service
run *args="":
    cargo run {{ build_flags }} -- {{ run_flags }} {{args}}

# Stratus: Compile with debug options
build:
    cargo build {{ build_flags }}

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
    SQLX_OFFLINE=true cargo sqlx prepare --database-url {{ database_url }} -- --all-targets
alias sqlx := db-compile

# ------------------------------------------------------------------------------
# Additional binaries
# ------------------------------------------------------------------------------

# Bin: Download external RPC blocks and receipts to temporary storage
rpc-downloader *args="":
    cargo run --bin rpc-downloader {{release_flag}} --features {{feature_flags}} -- {{args}}

# Bin: Import external RPC blocks from temporary storage to Stratus storage
importer-offline *args="":
    cargo run --bin importer-offline {{release_flag}} --features {{feature_flags}} -- {{args}}

# Bin: Import external RPC blocks from temporary storage to Stratus storage - with rocksdb
importer-offline-rocks *args="":
    cargo run --bin importer-offline {{release_flag}} --features rocks -- {{args}}

# Bin: Import external RPC blocks from external RPC endpoint to Stratus storage
importer-online *args="":
    cargo run --bin importer-online {{release_flag}} --features dev -- {{args}}

# Bin: Validate Stratus storage slots matches reference slots
state-validator *args="":
    cargo run --bin state-validator {{release_flag}} --features dev -- {{args}}

# Bin: `stratus` and `importer-online` in a single binary
run-with-importer *args="":
    cargo run --bin run-with-importer {{release_flag}} --features dev -- {{args}}

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
    cargo test --lib {{name}} --features {{feature_flags}} -- --nocapture

# Test: Execute Rust integration tests
test-int name="'*'":
    cargo test --test {{name}} {{release_flag}} --features {{feature_flags}},metrics,rocks -- --nocapture

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
        npx hardhat test test/{{block-mode}}/*.test.ts --network {{network}}
    else
        npx hardhat test test/{{block-mode}}/*.test.ts --network {{network}} --grep "{{test}}"
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
    wait-service --tcp 0.0.0.0:3000 -t {{ wait_service_timeout }} -- echo

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
    wait-service --tcp 0.0.0.0:3000 -t {{ wait_service_timeout }} -- echo

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
    cargo run  --release --bin stratus --features dev, -- --block-mode 1s -a 0.0.0.0:3000 > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{ wait_service_timeout }} -- echo

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
    cargo run  --release --bin stratus --features dev, -- --block-mode 1s --perm-storage=rocks -a 0.0.0.0:3000 > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{ wait_service_timeout }} -- echo

    echo "-> Validating block time"
    ./utils/block-time-check.sh
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000
    exit $result_code

# E2E: Lint and format code
e2e-lint:
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi
    node_modules/.bin/prettier . --write

# E2E: profiles rpc sync and generates a flamegraph
e2e-flamegraph:
    #!/bin/bash

    # Start PostgreSQL
    echo "Starting PostgreSQL"
    docker compose down -v
    docker compose up -d --force-recreate

    # Wait for PostgreSQL
    echo "Waiting for PostgreSQL to be ready"
    wait-service --tcp 0.0.0.0:5432 -t {{ wait_service_timeout }} -- echo
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
    wait-service --tcp 0.0.0.0:3003 -t {{ wait_service_timeout }} -- echo

    # Run cargo flamegraph with necessary environment variables
    echo "Running cargo flamegraph"
    cargo flamegraph --bin importer-online --deterministic --features dev,perf -- --external-rpc=http://localhost:3003/rpc --chain-id=2009

e2e-relayer:
    #!/bin/bash

    just e2e-relayer-external-up
    result_code=$?

    just e2e-relayer-external-down

    exit $result_code

# E2E: External Relayer job
e2e-relayer-external-up:
    #!/bin/bash

    # Build Stratus and Relayer binaries
    echo "Building Stratus binary"
    cargo build --release --bin stratus --features dev &
    echo "Building Relayer binary"
    cargo build --release --bin relayer --features dev &

    mkdir e2e_logs

    # Start Postgres
    docker-compose up -V -d

    # Wait for Postgres to start
    wait-service --tcp 0.0.0.0:5432 -t {{ wait_service_timeout }} -- echo

    # Wait for builds to finish
    wait

    # Start Stratus binary
    cargo run --release --bin stratus --features dev -- --block-mode 1s --perm-storage=rocks --relayer-db-url "postgres://postgres:123@0.0.0.0:5432/stratus" --relayer-db-connections 5 --relayer-db-timeout 1s -a 0.0.0.0:3000 > e2e_logs/stratus.log &

    # Wait for Stratus to start
    wait-service --tcp 0.0.0.0:3000 -t {{ wait_service_timeout }} -- echo

    # Install npm and start hardhat node in the e2e directory
    if [ -d e2e/cloudwalk-contracts ]; then
        (
            cd e2e/cloudwalk-contracts/integration
            npm install
            BLOCK_MODE=1s npx hardhat node > ../../../e2e_logs/hardhat.log &
        )
    fi

    # Wait for hardhat node to start
    wait-service --tcp 0.0.0.0:8545 {{ wait_service_timeout }} -- echo
    sleep 5

    # Start Relayer External binary
    cargo run --release --bin relayer --features dev -- --db-url postgres://postgres:123@0.0.0.0:5432/stratus --db-connections 5 --db-timeout 1s --forward-to http://0.0.0.0:8545 --stratus-rpc http://0.0.0.0:3000 --backoff 10ms --tokio-console-address 0.0.0.0:6979 --metrics-exporter-address 0.0.0.0:9001 > e2e_logs/relayer.log &

    if [ -d e2e/cloudwalk-contracts ]; then
    (
        cd e2e/cloudwalk-contracts/integration
        npx hardhat test test/*.test.ts --network stratus
        if [ $? -ne 0 ]; then
            echo "Hardhat tests failed"
            exit 1
        else
            echo "Hardhat tests passed successfully"
            exit 0
        fi
    )
    fi

# E2E: External Relayer job
e2e-relayer-external-down:
    #!/bin/bash

    # Kill relayer
    relayer_pid=$(pgrep -f 'relayer')
    kill $relayer_pid

    # Kill hardhat
    killport 8545

    # Kill Stratus
    killport 3000
    stratus_pid=$(pgrep -f 'stratus')
    kill $stratus_pid

    # Stop Postgres
    docker-compose down -v
    docker volume prune -f

    # Delete data contents
    rm -rf ./data/*

    # Recreate data directories
    mkdir -p data/mismatched_transactions
    rm -rf data/mismatched_transactions/*

    # Delete zeppelin directory
    rm -rf ./e2e/cloudwalk-contracts/integration/.openzeppelin

# ------------------------------------------------------------------------------
# Contracts tasks
# ------------------------------------------------------------------------------

# Contracts: Clone Solidity repositories
contracts-clone *args="":
    cd e2e/cloudwalk-contracts && ./clone-contracts.sh {{ args }}

# Contracts: Compile selected Solidity contracts
contracts-compile:
    cd e2e/cloudwalk-contracts && ./compile-contracts.sh

# Contracts: Flatten solidity contracts for integration test
contracts-flatten *args="":
    cd e2e/cloudwalk-contracts && ./flatten-contracts.sh {{ args }}

# Contracts: Test selected Solidity contracts on Stratus
contracts-test *args="":
    cd e2e/cloudwalk-contracts && ./test-contracts.sh {{ args }}
alias e2e-contracts := contracts-test

# Contracts: Remove all the cloned repositories
contracts-remove:
    cd e2e/cloudwalk-contracts && ./remove-contracts.sh

# Contracts: Start Stratus and run contracts test
contracts-test-stratus *args="":
    #!/bin/bash
    echo "-> Starting Stratus"
    just build || exit 1
    just run -a 0.0.0.0:3000 > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{ wait_service_timeout }} -- echo

    echo "-> Running E2E Contracts tests"
    just e2e-contracts {{ args }}
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000
    exit $result_code

contracts-test-stratus-rocks *args="":
    #!/bin/bash
    echo "-> Starting Stratus"
    just build || exit 1
    just run -a 0.0.0.0:3000 --perm-storage=rocks > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{ wait_service_timeout }} -- echo

    echo "-> Running E2E tests"
    just e2e-contracts {{ args }}
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000

    exit $result_code

# Contracts: Run tests and generate coverage info. Use --html to open in browser.
contracts-coverage *args="":
    cd e2e/cloudwalk-contracts && ./coverage-contracts.sh {{args}}

# Contracts: Erase coverage info
contracts-coverage-erase:
    #!/bin/bash
    cd e2e/cloudwalk-contracts/repos || exit 1
    echo "Erasing coverage info..."
    rm -rf ./*/coverage && echo "Coverage info erased."

# Chaos Testing: Set up and run local Kubernetes cluster and deploy locally the application
local-chaos-setup:
    @echo $(pwd)
    @echo "Installing dependencies..."
    ./chaos/install-dependencies.sh
    @echo "Checking if Kind cluster exists..."
    if ! kind get clusters | grep -q local-testing; then \
        echo "Setting up Kind cluster..."; \
        kind create cluster --name local-testing; \
        kind get kubeconfig --name local-testing > kubeconfig.yaml; \
    else \
        echo "Kind cluster already exists."; \
    fi
    @echo "Configuring kubectl to use Kind cluster..."
    export KUBECONFIG=$(pwd)/kubeconfig.yaml
    @echo "Checking if Docker image is already built..."
    if ! docker images | grep -q local/run_with_importer; then \
        echo "Building Docker image..."; \
        docker build -t local/run_with_importer -f ./docker/Dockerfile.run_with_importer .; \
    else \
        echo "Docker image already built."; \
    fi
    @echo "Loading Docker image into Kind..."
    kind load docker-image local/run_with_importer --name local-testing
    @echo "Deploying application..."
    kubectl apply -f chaos/local-deployment.yaml
    kubectl apply -f chaos/local-service.yaml
    @echo "Waiting for pods to be ready..."
    kubectl wait --for=condition=ready pod -l app=stratus-api --timeout=180s
    @echo "Deployment complete. Checking pod status..."
    kubectl get pods -o wide

# Chaos Testing: Clean up local Kubernetes cluster
local-chaos-cleanup:
    @echo "Deleting Kind cluster..."
    kind delete cluster --name local-testing
    @echo "Cleanup complete."

# Chaos Testing: Run chaos test
local-chaos-test:
    just local-chaos-setup
    @echo "Simulating leader polling and followers syncing for 60 seconds..."
    @for pod in $(kubectl get pods -l app=stratus-api -o jsonpath='{.items[*].metadata.name}'); do
    @    kubectl exec -it $$pod -- /bin/sh -c 'for i in {1..60}; do curl -s http://staging-endpoint/eth_getBlockByNumber?params=[latest,true]; sleep 1; done' &
    @done
    wait
    @echo "Leader polling and followers syncing simulated."
    @echo "Cleaning up..."
    just local-chaos-cleanup
