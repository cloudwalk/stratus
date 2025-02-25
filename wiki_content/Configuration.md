# Configuration Guide

## Node Configuration

### Mode Selection (Required, mutually exclusive)
- `--leader`: Run as leader node (processes transactions and mines blocks)
- `--follower`: Run as follower node (syncs blocks from leader)
- `--fake-leader`: Run as fake leader (imports blocks like follower, executes locally like leader)

### Common Settings
- `--env`: Environment (local, staging, production, canary)
- `--async-threads`: Number of threads for global async tasks (default: 32)
- `--blocking-threads`: Number of threads for global blocking tasks (default: 512)
- `--unknown-client-enabled`: Enable/disable unknown client interactions (default: true)

### Storage Options
Stratus supports multiple storage backends and types for different use cases:

#### Storage Backends
- In-Memory: Fast, volatile storage for development and testing
  - Ideal for local development and testing
  - Data is lost when the node restarts
  - No disk space requirements
- RocksDB: Persistent storage for production environments (default)
  - Optimized for blockchain state storage
  - Provides durability and crash recovery
  - Efficient key-value storage with compression

#### Storage Types
- Temporary Storage
  - Purpose: Manages in-progress blocks during mining
  - Use cases:
    - Transaction pool management
    - Block assembly and validation
    - Intermediate state changes
  - Cleared after block finalization

- Permanent Storage
  - Purpose: Long-term blockchain data storage
  - Components stored:
    - Account states and balances
    - Contract bytecode and storage
    - Historical blocks and receipts
    - Transaction history
  - Ensures data consistency and availability

- Cache Storage
  - Purpose: Improves read performance for frequently accessed data
  - Components:
    - Slot Cache: Contract storage slots (default: 100,000 entries)
    - Account Cache: Account states and balances (default: 20,000 entries)
  - Configurable capacity and eviction policies
  - Automatically updates with state changes

### Executor Configuration

#### EVM Settings
- `EXECUTOR_CHAIN_ID` (alias: `CHAIN_ID`): Chain ID for the network
  - Example: `1` for Ethereum Mainnet, `2009` for test networks
  - Required: Yes
  - Default: None, must be specified

- `EXECUTOR_EVMS` (alias: `EVMS`, `NUM_EVMS`): Number of EVM instances
  - Purpose: Controls parallel transaction processing capacity
  - Default: 30
  - Range: 1-1000
  - Note: Higher values increase throughput but require more memory

- `EXECUTOR_STRATEGY`: Execution strategy selection
  - Options:
    - `serial` (default): Single-threaded execution for deterministic order
    - `parallel`: Multi-threaded execution for better performance
  - Impact: Affects transaction ordering and throughput
  - Use Case: Choose `serial` for predictable execution, `parallel` for higher performance

- `EXECUTOR_REJECT_NOT_CONTRACT`: Reject transactions to non-contract addresses
  - Values: `true`/`false`
  - Default: `true`
  - Purpose: Controls whether to allow transactions to EOA (Externally Owned Accounts)

- `EXECUTOR_EVM_SPEC`: EVM specification version
  - Default: `Cancun`
  - Available: `Frontier`, `Homestead`, `Byzantium`, `Constantinople`, `Petersburg`, `Istanbul`, `Berlin`, `London`, `Paris`, `Shanghai`, `Cancun`
  - Purpose: Sets the EVM rules and features to use

### Monitoring
- `TRACING_LOG_FORMAT` (alias: `LOG_FORMAT`): Log format configuration
  - Options: `json`, `text`
  - Default: `json`
  - Example: `TRACING_LOG_FORMAT=json`
  - Purpose: Controls the format of log output for easier parsing or human readability

- `TRACING_URL` (alias: `TRACING_COLLECTOR_URL`): OpenTelemetry collector URL
  - Format: `http://<host>:<port>`
  - Default: None (tracing disabled if not set)
  - Example: `TRACING_URL=http://localhost:4317`
  - Purpose: Endpoint for sending OpenTelemetry traces and metrics
  - Note: Required for distributed tracing and monitoring

## RPC Downloader Configuration
The RPC Downloader is responsible for fetching blockchain data from external RPC nodes. It downloads blocks and transaction receipts to temporary PostgreSQL storage for later import.

### Settings
- `--block-end`: Final block number to download
  - Purpose: Specifies the last block to download
  - Format: Integer block number
  - Required: Yes

- `--external-rpc`: External RPC endpoint URL
  - Purpose: Source blockchain node to download from
  - Format: `http://<host>:<port>`
  - Required: Yes
  - Example: `http://localhost:8545`

- `--external-rpc-timeout`: Timeout for blockchain requests
  - Purpose: Controls request timeout duration
  - Default: 2 seconds
  - Format: Duration in seconds
  - Example: `5` for 5 seconds timeout

- `--paralellism`: Number of parallel downloads
  - Purpose: Controls download concurrency
  - Default: 1
  - Range: 1-100
  - Note: Higher values increase throughput but use more resources

- `--initial-accounts`: Comma-separated list of accounts
  - Purpose: Retrieve initial balance for specified accounts
  - Format: Comma-separated Ethereum addresses
  - Example: `0x123...,0x456...`
  - Optional: Yes

- `--external-rpc-storage`: PostgreSQL connection string
  - Purpose: Temporary storage for downloaded data
  - Format: `postgres://<user>:<password>@<host>:<port>/<database>`
  - Required: Yes

- `--metrics-exporter-address`: Metrics endpoint address
  - Purpose: Exposes Prometheus metrics
  - Format: `<host>:<port>`
  - Optional: Yes
  - Example: `0.0.0.0:9001`

## Importer Offline Configuration
The Importer Offline module imports blockchain data from temporary storage (PostgreSQL) into Stratus's permanent storage (RocksDB). This allows for offline data migration and recovery.

### Settings
- `--block-start`: Initial block number to import
  - Purpose: First block to import from temporary storage
  - Format: Integer block number
  - Required: Yes

- `--block-end`: Final block number to import
  - Purpose: Last block to import from temporary storage
  - Format: Integer block number
  - Required: Yes

- `--paralellism`: Number of parallel database fetches
  - Purpose: Controls import concurrency
  - Default: 1
  - Range: 1-100
  - Note: Higher values increase throughput but use more memory

- `--blocks-by-fetch`: Number of blocks per database fetch
  - Purpose: Controls batch size for each import operation
  - Default: 10000
  - Range: 1-50000
  - Note: Larger batches are more efficient but use more memory

- `--external-rpc-storage`: PostgreSQL connection string
  - Purpose: Source database containing downloaded blocks
  - Format: `postgres://<user>:<password>@<host>:<port>/<database>`
  - Required: Yes

- `--rocks-path-prefix`: Path prefix for RocksDB storage
  - Purpose: Destination for imported blockchain data
  - Format: Directory path
  - Required: Yes
  - Example: `data/importer-offline-database`

- `--metrics-exporter-address`: Metrics endpoint address
  - Purpose: Exposes Prometheus metrics
  - Format: `<host>:<port>`
  - Optional: Yes
  - Example: `0.0.0.0:9002`

## Performance Tuning
See [Performance Optimization Guide](Performance-Optimization) for detailed tuning parameters.
