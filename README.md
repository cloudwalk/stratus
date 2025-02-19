[![codecov](https://codecov.io/github/cloudwalk/stratus/graph/badge.svg?token=D1795GHLG6)](https://codecov.io/github/cloudwalk/stratus)

# Stratus ☁️

Welcome to Stratus, the cutting-edge EVM executor and JSON-RPC server with custom storage that scales horizontally. Built in Rust 🦀 for speed and reliability, Stratus is here to take your blockchain projects to the next level!

## Architecture Overview

Stratus is built on several key components:
- EVM Executor: Processes smart contract transactions
- JSON-RPC Server: Provides API access to the blockchain
- Custom Storage Layer: Supports horizontal scaling
  - In Memory (development)
  - RocksDB (production default)
- OpenTelemetry Integration: Comprehensive monitoring

## Performance Landscape

Stratus is engineered for high performance, with a unique node Stratus was capable of handling:

- 10k transactions per second (TPS) for reading
- 1,8k transactions per second (TPS) for writing (custom contract operations), reaching around 360M of gas per second

We aim to reach 5k write transactions per second with a unique node and 1M in a cluster.

## Getting Started with Stratus

### Prerequisites
- [Rust](https://www.rust-lang.org/tools/install) - Latest stable version
- [just](https://github.com/casey/just) - Command runner for development tasks
- Git - For version control
- [asdf](https://asdf-vm.com/) - Version manager (required for testing)

### Quick Start
1. Clone and enter the repository:
   ```bash
   git clone https://github.com/cloudwalk/stratus.git
   cd stratus
   ```

2. Set up the development environment:
   ```bash
   just setup
   ```

3. Run Stratus in optimized mode:
   ```bash
   RELEASE=1 just run
   ```

### Monitoring
Stratus includes comprehensive monitoring through OpenTelemetry:
- Use `--tracing-url <url>` to specify your collector
- Default Jaeger collector: `http://localhost:4317`
- Access Jaeger UI at `localhost:16686`

### Troubleshooting
- Ensure all prerequisites are installed
- Check the logs for detailed error messages
- Refer to our [wiki](https://github.com/cloudwalk/stratus/wiki) for detailed guides

### Testing

To run tests, you also need to:

- Install Git
- Install [asdf version manager](https://asdf-vm.com/) and use it to install
  + Node.js `v20.10.0` and `v21.6.1`
  + Solidity `v0.8.16`

Configure the test environment with

```bash
just setup
```

Then run one of test recipes we provide. You can `just | grep test` to see them.
To see all available tasks you can simply run `just`.

We recommend using just recipes whenever applicable.

## Join the Party

We love contributions! Check out our [Contributing Guide](CONTRIBUTING.md) and help make Stratus even more awesome.

## License

Stratus is licensed under the MIT license.
