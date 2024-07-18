# Stratus ☁️

Welcome to Stratus, the cutting-edge EVM executor and JSON-RPC server with custom storage that scales horizontally. Built in Rust 🦀 for speed and reliability, Stratus is here to take your blockchain projects to the next level!

## Current storage implementations

- In Memory
- PostgreSQL
- RocksDB (default)

## Performance Landscape

Stratus is engineered for high performance, with a unique node Stratus was capable of handling:

- 10k transactions per second (TPS) for reading
- 1,8k transactions per second (TPS) for writing (custom contract operations), reaching around 32M of gas per second

We aim to reach 5k write transactions per second with a unique node and 1M in a cluster.

## Getting Started with Stratus

To run the optimized version of Stratus, ensure you have installed the dependencies:

- [Rust](https://www.rust-lang.org/tools/install)
- [just](https://github.com/casey/just)

Then simply run:

```bash
RELEASE=1 just run
```

If you want to use OpenTelemery use the flag `--tracing-url <url>` and pass
the url of your OpenTelemetry collector of choice. Jaeger is included in the compose
file, its collector url is `http://localhost:4317` and the ui can be accessed at
`localhost:16686` on your browser.

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
