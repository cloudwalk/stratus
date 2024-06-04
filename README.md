# Stratus ☁️

Stratus is an EVM executor and JSON-RPC server with custom storage that scales horizontally, written in Rust 🦀.

## Current storage implementations

- In Memory
- PostgreSQL
- RocksDB (default)

## Contributing

Check our [`CONTRIBUTING.md`](CONTRIBUTING.md) to get started.

## Getting Started with Stratus

To run the optimized version of Stratus, ensure you have installed the dependencies:

- [Rust](https://www.rust-lang.org/tools/install)
- [just](https://github.com/casey/just)

Then simply run:

```bash
RELEASE=1 just run
```

If you want to use OpenTelemery use the flag `--tracing-collector-url <url>` and pass
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

## Wiki

Check [our wiki](https://github.com/cloudwalk/stratus/wiki) for more info in specific subjects.

## License

Stratus is licensed under the MIT license.
