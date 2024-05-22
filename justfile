import '.justfile_helpers' # _lint, _outdated

# Environment variables automatically passed to executed commands.
export CARGO_PROFILE_RELEASE_DEBUG := env("CARGO_PROFILE_RELEASE_DEBUG", "1")
export RUST_BACKTRACE := "0"
export RUST_LOG := env("RUST_LOG", "stratus=info,rpc_downloader=info,importer_offline=info,importer_online=info,state_validator=info")

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

# Database: Load CSV data produced by importer-offline
db-load-csv:
    echo "" > data/psql.txt

    echo "truncate accounts;"            >> data/psql.txt
    echo "truncate historical_nonces;"   >> data/psql.txt
    echo "truncate historical_balances;" >> data/psql.txt
    echo "truncate historical_slots;"    >> data/psql.txt
    echo "truncate blocks;"              >> data/psql.txt
    echo "truncate transactions;"        >> data/psql.txt
    echo "truncate logs;"                >> data/psql.txt

    ls -tr1 data/accounts-*.csv            | xargs -I{} printf "\\\\copy accounts            from '$(pwd)/%s' delimiter E'\\\\t' csv header;\n" "{}" >> data/psql.txt
    ls -tr1 data/historical_nonces-*.csv   | xargs -I{} printf "\\\\copy historical_nonces   from '$(pwd)/%s' delimiter E'\\\\t' csv header;\n" "{}" >> data/psql.txt
    ls -tr1 data/historical_balances-*.csv | xargs -I{} printf "\\\\copy historical_balances from '$(pwd)/%s' delimiter E'\\\\t' csv header;\n" "{}" >> data/psql.txt
    ls -tr1 data/historical_slots-*.csv    | xargs -I{} printf "\\\\copy historical_slots    from '$(pwd)/%s' delimiter E'\\\\t' csv header;\n" "{}" >> data/psql.txt
    ls -tr1 data/blocks-*.csv              | xargs -I{} printf "\\\\copy blocks              from '$(pwd)/%s' delimiter E'\\\\t' csv header;\n" "{}" >> data/psql.txt
    ls -tr1 data/transactions-*.csv        | xargs -I{} printf "\\\\copy transactions        from '$(pwd)/%s' delimiter E'\\\\t' csv header;\n" "{}" >> data/psql.txt
    ls -tr1 data/logs-*.csv                | xargs -I{} printf "\\\\copy logs                from '$(pwd)/%s' delimiter E'\\\\t' csv header;\n" "{}" >> data/psql.txt

    cat data/psql.txt | pgcli -h localhost -u postgres -d stratus --less-chatty

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
e2e network="stratus" test="":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi
    if [ ! -d node_modules ]; then
        npm install
    fi

    if [ -z "{{test}}" ]; then
        npx hardhat test test/*.test.ts --network {{network}}
    else
        npx hardhat test test/*.test.ts --network {{network}} --grep "{{test}}"
    fi

# E2E: Starts and execute Hardhat tests in Hardhat
e2e-hardhat test="":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi

    echo "-> Starting Hardhat"
    npx hardhat node &

    echo "-> Waiting Hardhat to start"
    wait-service --tcp localhost:8545 -- echo

    echo "-> Running E2E tests"
    just e2e hardhat {{test}}

    echo "-> Killing Hardhat"
    killport 8545

# E2E: Starts and execute Hardhat tests in Stratus
e2e-stratus test="":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi

    echo "-> Starting Stratus"
    just build || exit 1
    just run -a 0.0.0.0:3000 > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{ wait_service_timeout }} -- echo

    echo "-> Running E2E tests"
    just e2e stratus {{test}}
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000
    exit $result_code

# E2E: Starts and execute Hardhat tests in Stratus
e2e-stratus-rocks test="":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi

    echo "-> Starting Stratus"
    just build || exit 1
    just run -a 0.0.0.0:3000 --perm-storage=rocks > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{ wait_service_timeout }} -- echo

    echo "-> Running E2E tests"
    just e2e stratus {{test}}
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000
    exit $result_code

# E2E: Starts and execute Hardhat tests in Stratus
e2e-stratus-postgres test="":
    #!/bin/bash
    if [ -d e2e ]; then
        cd e2e
    fi

    echo "-> Starting Postgres"
    docker compose down
    docker compose up -d || exit 1

    echo "-> Waiting Postgres to start"
    wait-service --tcp 0.0.0.0:5432 -t {{ wait_service_timeout }} -- echo

    echo "-> Starting Stratus"
    just build || exit 1
    just run -a 0.0.0.0:3000 --perm-storage {{ database_url }} > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{ wait_service_timeout }} -- echo

    echo "-> Running E2E tests"
    just e2e stratus {{test}}
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000

    echo "-> Killing Postgres"
    docker compose down

    echo "** -> Stratus log accessible in ./stratus.log **"
    exit $result_code

# E2E Clock: Builds and runs Stratus with block-time flag, then validates average block generation time
e2e-clock-stratus:
    #!/bin/bash
    echo "-> Starting Stratus"
    just build || exit 1
    cargo run  --release --bin stratus --features dev, -- --block-time 1000 -a 0.0.0.0:3000 > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{ wait_service_timeout }} -- echo

    echo "-> Validating block time"
    just block-time-check
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000
    exit $result_code

# E2E Clock: Builds and runs Stratus Rocks with block-time flag, then validates average block generation time
e2e-clock-stratus-rocks:
    #!/bin/bash
    echo "-> Starting Stratus"
    just build || exit 1
    cargo run  --release --bin stratus --features dev, -- --block-time 1000 --perm-storage=rocks -a 0.0.0.0:3000 > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{ wait_service_timeout }} -- echo

    echo "-> Validating block time"
    just block-time-check
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000
    exit $result_code

# Checks if the average block time is within the expected range
block-time-check:
    #!/bin/bash
    sleep_interval=180
    error_margin=0.05

    echo -n "-> Waiting for blocks to generate... "
    for ((i=1; i<=$sleep_interval; i++)); do
        printf "\r-> Waiting for blocks to generate... %d/%ds" $i $sleep_interval
        sleep 1
    done

    echo "-> Getting info on first block"
    first_block_info=$(curl -s http://0.0.0.0:3000 \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["earliest",false],"id":1}')

    echo "-> Getting info on latest block"
    latest_block_info=$(curl -s http://0.0.0.0:3000 \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["latest",false],"id":1}')

    # Extract block time and block number
    first_block_time_hex=$(echo ${first_block_info} | jq -r '.result.timestamp')
    latest_block_time_hex=$(echo ${latest_block_info} | jq -r '.result.timestamp')
    block_count_hex=$(echo ${latest_block_info} | jq -r '.result.number')

    # Remove 0x prefix
    first_block_time_hex_no_prefix=${first_block_time_hex#0x}
    latest_block_time_hex_no_prefix=${latest_block_time_hex#0x}
    block_count_hex_no_prefix=${block_count_hex#0x}

    # Convert hex to number
    first_block_time_dec=$((16#${first_block_time_hex_no_prefix}))
    latest_block_time_dec=$((16#${latest_block_time_hex_no_prefix}))  
    block_count=$((16#${block_count_hex_no_prefix}))

    # Elapsed time between first and latest block
    block_time_elapsed=$(($latest_block_time_dec - $first_block_time_dec))

    # Amount of blocks mined during the elapsed time
    blocks_mined=$((block_count - 1))

    # Average block generation time
    average_block_time=$(echo "scale=2; $block_time_elapsed / $blocks_mined" | bc)
    echo "-> Average block generation time: $average_block_time s"

    # Calculate the absolute value of the difference between the average block time and the expected block time
    actual_error_margin=$(echo "$average_block_time - 1" | bc)
    if (( $(echo "$actual_error_margin < 0" | bc -l) )); then
        actual_error_margin=$(echo "$actual_error_margin * -1" | bc)
    fi

    # Check if the difference is within the acceptable range
    if (( $(echo "$actual_error_margin <= $error_margin" | bc -l) )); then
        echo "Average block time is within the acceptable range: $average_block_time per second"
        exit 0
    fi
    echo "Error: Average block time is not within the acceptable range"
    exit 1

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

# ------------------------------------------------------------------------------
# Contracts tasks
# ------------------------------------------------------------------------------

# Contracts: Clone Solidity repositories
contracts-clone *args="":
    cd e2e-contracts && ./clone-contracts.sh {{ args }}

# Contracts: Compile selected Solidity contracts
contracts-compile:
    cd e2e-contracts && ./compile-contracts.sh

# Contracts: Flatten solidity contracts for integration test
contracts-flatten:
    cd e2e-contracts && ./flatten-contracts.sh

# Contracts: Test selected Solidity contracts on Stratus
contracts-test *args="":
    cd e2e-contracts && ./test-contracts.sh {{ args }}
alias e2e-contracts := contracts-test

# Contracts: Run BRLCToken contract tests
contracts-test-brlc-token:
    cd e2e-contracts && ./test-contracts.sh -t

# Contracts: Run BRLCPeriphery contract tests
contracts-test-brlc-periphery:
    cd e2e-contracts && ./test-contracts.sh -p

# Contracts: Run BRLCMultisig contract tests
contracts-test-brlc-multisig:
    cd e2e-contracts && ./test-contracts.sh -m

# Contracts: Run CompoundPeriphery contract tests
contracts-test-brlc-compound:
    cd e2e-contracts && ./test-contracts.sh -c

# Contracts: Remove all the cloned repositories
contracts-remove:
    cd e2e-contracts && ./remove-contracts.sh

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

# Contracts: Start Stratus with Postgres and run contracts test
contracts-test-stratus-postgres *args="":
    #!/bin/bash
    echo "-> Starting Postgres"
    docker compose down
    docker compose up -d || exit 1

    echo "-> Waiting Postgres to start"
    wait-service --tcp 0.0.0.0:5432 -t {{ wait_service_timeout }} -- echo

    echo "-> Starting Stratus"
    just build || exit 1
    just run -a 0.0.0.0:3000 --perm-storage {{ database_url }} > stratus.log &

    echo "-> Waiting Stratus to start"
    wait-service --tcp 0.0.0.0:3000 -t {{ wait_service_timeout }} -- echo

    echo "-> Running E2E tests"
    just e2e-contracts {{ args }}
    result_code=$?

    echo "-> Killing Stratus"
    killport 3000

    echo "-> Killing Postgres"
    docker compose down

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

# Contracts: run contract integration tests
contracts-test-int:
    #!/bin/bash
    cd e2e-contracts && ./flatten-contracts.sh
    [ -d integration ] && cd integration
    [ ! -f hardhat.config.ts ] && { cp ../../e2e/hardhat.config.ts .; }
    [ ! -f tsconfig.json ] && { cp ../../e2e/tsconfig.json .; }
    if [ ! -d node_modules ]; then
        echo "Installing node modules"
        npm --silent install hardhat@2.21.0 ethers@6.11.1 @openzeppelin/hardhat-upgrades @openzeppelin/contracts-upgradeable @nomicfoundation/hardhat-ethers @nomicfoundation/hardhat-toolbox @nomicfoundation/hardhat-ethers
        command -v ts-node >/dev/null 2>&1 || { npm install --silent -g ts-node; }
    fi
    npx hardhat test
    exit $?

# Contracts: Run tests and generate coverage info. Use --html to open in browser.
contracts-coverage *args="":
    cd e2e-contracts && ./coverage-contracts.sh {{args}}

# Contracts: Erase coverage info
contracts-coverage-erase:
    #!/bin/bash
    cd e2e-contracts/repos
    rm -rf */coverage

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
