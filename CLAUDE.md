# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Stratus is a high-performance EVM-compatible blockchain infrastructure written in Rust, designed as an EVM executor and JSON-RPC server with custom storage capabilities. It supports 10k TPS for reads and 2.5k TPS for writes, with a distributed leader-follower architecture.

## Development Commands

### Core Commands
- `just build` - Build Stratus with debug options (default: dev features)
- `just build stratus release` - Build optimized release version
- `just check` - Check compilation without generating code
- `just lint` - Format and lint all code (Rust + TypeScript)
- `just lint-check` - Check formatting and run clippy with strict warnings
- `just test` - Run Rust tests with coverage reporting
- `just clean` - Clean build artifacts

### Running Stratus
- `just stratus` or `just run` - Run as leader node (development mode)
- `just stratus-follower` - Run as follower node
- `RELEASE=1 just run` - Run optimized release version
- `just stratus-test` - Run with test coverage instrumentation

### Database & Storage
- `just db-compile` or `just sqlx` - Compile SQLx queries offline
- Database URL: `postgres://postgres:123@0.0.0.0:5432/stratus` (default)
- SQL is only used for the rpc_downloader and importer-offline binaries
- Storage options: RocksDB (default) or In-Memory

### Testing
- `just test` - Unit tests with coverage
- `just e2e-stratus` - End-to-end tests with Stratus
- `just e2e-hardhat` - End-to-end tests with Hardhat
- `just e2e-leader-follower-up` - Leader-follower integration tests
- `just contracts-test-stratus` - CloudWalk contracts testing
- `just e2e-genesis` - Genesis configuration tests

### Additional Binaries
- `just rpc-downloader` - Download external RPC blocks to temp storage
- `just importer-offline` - Import external RPC blocks to Stratus storage

## Architecture

### Core Components
- **`src/eth/`** - Ethereum functionality
  - `executor/` - EVM execution engine using REVM
  - `rpc/` - JSON-RPC server implementation
  - `storage/` - Storage layer (RocksDB + In-Memory)
  - `primitives/` - Ethereum primitive types
  - `miner/` - Block mining functionality
  - `follower/` - Follower node with consensus and importer
  - `external_rpc/` - External blockchain RPC with PostgreSQL
- **`src/infra/`** - Infrastructure (metrics, tracing, Kafka, Sentry)
- **`src/ledger/`** - Ledger event management
- **`e2e/`** - TypeScript/Hardhat end-to-end tests

### Key Features
- Leader-follower distributed architecture
- EVM execution with REVM integration
- RocksDB storage with replication support
- Kafka integration for event streaming
- Comprehensive observability (Prometheus, OpenTelemetry, Grafana)
- Full Ethereum JSON-RPC compatibility

## Code Standards

### Rust Configuration
- **Rust version**: 1.86 (specified in rust-toolchain.toml)
- **Max line width**: 160 characters
- **Import style**: Item-level granularity, grouped by StdExternalCrate
- **Formatting**: Use `just lint` which runs `cargo fmt` and `cargo clippy`
- **Linting**: Warnings treated as errors, but `unwrap()`, `expect()`, `panic!()` allowed in tests

### Key Clippy Rules
- `unwrap_used = "allow"` - Allowed in all contexts
- `expect_used = "warn"` - Discouraged but not forbidden
- `panic = "warn"` - Discouraged but not forbidden
- `disallowed_names = "warn"` - Avoid variable names like "lock"
- `wildcard_imports = "warn"` - Avoid `use module::*`

### Features and Build
- Default features: `["metrics", "tracing"]`
- Development: Use `--features dev` for development builds
- Release: Use `--features dev --release` for optimized development builds
- Special features: `jeprof` (memory profiling), `flamegraph`, `jemalloc`

## Testing Strategy

### Test Types
1. **Unit tests**: `cargo test` with coverage via `cargo llvm-cov`
2. **E2E tests**: Hardhat/TypeScript tests against running Stratus instance
3. **Integration tests**: CloudWalk contracts testing
4. **Performance tests**: TPS benchmarks and profiling
5. **Leader-follower tests**: Distributed consensus testing

### Test Environment
- **Node.js**: v20.10.0 and v21.6.1 (use asdf)
- **Solidity**: v0.8.16
- **Setup**: Run `just setup` to install dependencies
- **Infrastructure**: Use `docker-compose.yaml` for PostgreSQL, Kafka, etc.

### Running Tests
- Always use `just` recipes for consistent test execution
- Tests automatically instrument code for coverage
- E2E tests start/stop Stratus instances automatically
- Use `just run-test <recipe>` for tests with coverage and cleanup

## Dependencies and Tools

### Required Tools
- Rust (1.86)
- just (task runner)
- Git
- Docker & Docker Compose
- Node.js (via asdf)

### Optional Tools (installed via `just setup`)
- `cargo killport` - Kill processes on specific ports
- `cargo wait-service` - Wait for services to be available
- `cargo flamegraph` - Performance profiling

## Configuration

### Environment Files
- `config/` - Environment-specific configurations
- `LOCAL_ENV_PATH` - Override environment file path
- `.env` files supported via `dotenvy`

### Key Environment Variables
- `RUST_BACKTRACE` - Backtrace verbosity (default: 0)
- `RELEASE` - Build in release mode when set to 1
- `NIGHTLY` - Use nightly toolchain when set to 1
- `DATABASE_URL` - PostgreSQL connection string

## Performance Targets

- **Read TPS**: 10,000 transactions per second
- **Write TPS**: 2,500 transactions per second (~540M gas/second)
- **Future goals**: 5k TPS single node, 1M TPS clustered

## Monitoring

- **Metrics**: Prometheus metrics at `/metrics` endpoint
- **Tracing**: OpenTelemetry with Jaeger (UI at localhost:16686)
- **Logs**: Structured logging with tracing-subscriber
- **Error tracking**: Sentry integration
- **Dashboards**: Grafana dashboards included

## Development Best Practices

- Always run `cargo check` after making changes

## Storage Best Practices

- When editing StratusStorage pay attention to the transient_state_lock contract, "Always acquire a lock when reading slots or accounts from latest (cache OR perm) and when saving a block"