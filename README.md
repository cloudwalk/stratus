# Stratus ☁️

Welcome to Stratus, the cutting-edge EVM-compatible ledger and JSON-RPC server. This projects has 3 focuses of optimization:

1. Reliability: We have nodes running for months without any sort of restart or maintenance, and we give you the tools needed to do maintenance on the leader node with less than 5 seconds of downtime.
2. Observability: We have a lot of metric, logs, tracing implemented so that you can effectively monitor every aspect of the application.
3. Performance: By not aiming to be a blockchain (i.e. no decentralization) and by optimizing for scenarios with a limited number of contracts, we've been able to get single-token performances unheardof in any other evm-compatible ledger*. Currently we can achieve around 500M gas per second**.

You'd be hard pressed to find a private evm-compatible ledger solution that achieves those 3 factors as well as we do.

*: Although you can find some that can do more GPS than us, it is measured on scenarios were the contracts are completely independent, such as n separate erc20-tokens. These types of executions can be parallelized, but we removed this feature from stratus in favor of optimizing for scenarios where transactions are mostly dependant on one another.


**: This varies greatly depending on the contract implementation. Since gas costs are stipulated for traditional blockchains, gas throughput is not the ideal proxy for a performance metric on Stratus.

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

### Continuous integration

Public heavy checks are lightweight proxies: they dispatch the checked-out Stratus commit to private workflows in `cloudwalk/stratus-internal-builds` and report the result under the existing public check names. Fork pull requests cannot dispatch private CI. A maintainer can push the exact fork commits to a `cloudwalk/stratus` branch and rerun the checks from there.

Kafka E2E tests use `E2E_KAFKA_BOOTSTRAP_SERVERS` (default `localhost:29092`). CI environments that provide Kafka externally should set `KAFKA_MANAGED_EXTERNALLY=1`; this skips local Compose startup, topic creation, and teardown. The private CI repository provides its Kafka service at `kafka:9092`.

## Join the Party

We love contributions! Check out our [Contributing Guide](CONTRIBUTING.md) and help make Stratus even more awesome.

## License

Stratus is licensed under the MIT license.
