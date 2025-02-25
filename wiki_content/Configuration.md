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
Stratus supports multiple storage backends for different use cases:

#### Storage Backends
- In-Memory: Fast, volatile storage for development and testing
- RocksDB: Persistent storage for production environments (default)

#### Storage Types
- Temporary Storage: Used for short-lived data during transaction processing
- Permanent Storage: Stores blockchain state, blocks, and transactions
- Cache Storage: Improves performance by caching frequently accessed data

### Executor Configuration
- `EXECUTOR_CHAIN_ID` (alias: `CHAIN_ID`): Chain ID for the network (e.g., 1 for Ethereum Mainnet)
- `EXECUTOR_EVMS` (alias: `EVMS`, `NUM_EVMS`): Number of EVM instances (default: 30)
- `EXECUTOR_REJECT_NOT_CONTRACT`: Reject transactions to non-contract addresses (true/false)
- `EXECUTOR_STRATEGY`: Execution strategy selection (options: serial, parallel)

### Monitoring
- `TRACING_LOG_FORMAT` (alias: `LOG_FORMAT`): Log format configuration (options: json, text)
  Example: `TRACING_LOG_FORMAT=json`
- `TRACING_URL` (alias: `TRACING_COLLECTOR_URL`): OpenTelemetry collector URL
  Example: `TRACING_URL=http://localhost:4317`
- `TRACING_LOG_FORMAT` (alias: `LOG_FORMAT`): Log format configuration
- `TRACING_URL` (alias: `TRACING_COLLECTOR_URL`): OpenTelemetry collector URL

## RPC Downloader Configuration
The RPC Downloader is responsible for fetching blockchain data from external RPC nodes. It's used to sync historical blocks and state.

### Settings
- `--block-end`: Final block number to download
- `--external-rpc`: External RPC endpoint URL
- `--external-rpc-timeout`: Timeout for blockchain requests (default: 2s)
- `--paralellism`: Number of parallel downloads (default: 1)
- `--initial-accounts`: Comma-separated list of accounts to retrieve initial balance

## Importer Offline Configuration
The Importer Offline module allows importing blockchain data from local storage without requiring an active network connection. This is useful for data migration and recovery scenarios.
- `--block-start`: Initial block number to import
- `--block-end`: Final block number to import
- `--paralellism`: Number of parallel database fetches (default: 1)
- `--blocks-by-fetch`: Number of blocks per database fetch (default: 10000)

## Performance Tuning
See [Performance Optimization Guide](Performance-Optimization) for detailed tuning parameters.
