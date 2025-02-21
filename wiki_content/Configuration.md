# Configuration Guide

## Node Configuration

### Mode Selection
- `--leader`: Run as leader node
- `--follower`: Run as follower node
- `--fake-leader`: Run as fake leader (imports blocks like follower, executes locally like leader)

### Common Settings
- `--env`: Environment (local, staging, production, canary)
- `--async-threads`: Number of threads for global async tasks (default: 32)
- `--blocking-threads`: Number of threads for global blocking tasks (default: 512)
- `--unknown-client-enabled`: Enable/disable unknown client interactions (default: true)

## Stratus Node Configuration
### Mode Selection (Required, mutually exclusive)
- `--leader`: Run as leader node
- `--follower`: Run as follower node
- `--fake-leader`: Run as fake leader (imports blocks like follower, executes locally like leader)

### Storage Options
- In-Memory (development)
- RocksDB (production default)

### Executor Configuration
- `EXECUTOR_CHAIN_ID` (alias: `CHAIN_ID`): Chain ID for the network
- `EXECUTOR_EVMS` (alias: `EVMS`, `NUM_EVMS`): Number of EVM instances
- `EXECUTOR_REJECT_NOT_CONTRACT`: Reject transactions to non-contract addresses
- `EXECUTOR_STRATEGY`: Execution strategy selection

### Monitoring
- `TRACING_LOG_FORMAT` (alias: `LOG_FORMAT`): Log format configuration
- `TRACING_URL` (alias: `TRACING_COLLECTOR_URL`): OpenTelemetry collector URL

## RPC Downloader Configuration
- `--block-end`: Final block number to download
- `--external-rpc`: External RPC endpoint URL
- `--external-rpc-timeout`: Timeout for blockchain requests (default: 2s)
- `--paralellism`: Number of parallel downloads (default: 1)
- `--initial-accounts`: Comma-separated list of accounts to retrieve initial balance

## Importer Offline Configuration
- `--block-start`: Initial block number to import
- `--block-end`: Final block number to import
- `--paralellism`: Number of parallel database fetches (default: 1)
- `--blocks-by-fetch`: Number of blocks per database fetch (default: 10000)

## Performance Tuning
See [Performance Optimization Guide](Performance-Optimization) for detailed tuning parameters.
