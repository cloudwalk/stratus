# Stratus ☁️

Stratus is an EVM executor and JSON-RPC server with custom storage that scales horizontally, written in Rust 🦀.

## Current storage implementations

- In Memory
- PostgreSQL

## Contributing

Check our `CONTRIBUTING.md` to get started.

## Getting Started with Stratus
To run the optimized version of Stratus, ensure you have installed the dependencies:

- [Rust](https://www.rust-lang.org/tools/install)
- [just](https://github.com/casey/just)

Then simply run:

```bash
just run-release
```

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

## License

Stratus is licensed under the MIT license.
