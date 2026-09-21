//! Configuration file and CLI merge semantics, in-process over the merged configuration.

use std::ffi::OsString;

use stratus::config::Environment;
use stratus::config::StratusConfig;
use stratus::eth::miner::MinerMode;

/// Parses CLI arguments over the given config file content and builds the merged configuration.
fn load_with(args: &[&str], file_content: &str) -> anyhow::Result<StratusConfig> {
    let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
    StratusConfig::load_from(&argv, file_content)
}

#[test]
fn test_cli_overrides_file() {
    let file = r#"
        leader = true

        [executor]
        chain_id = 2008
        call_present_evms = 11

        [rpc]
        address = "0.0.0.0:3001"

        [miner]
        block_mode = "1s"
    "#;

    // no CLI args: file values are kept
    let config = load_with(&[], file).unwrap();
    assert!(config.leader);
    assert_eq!(config.executor.executor_chain_id, 2008);
    assert_eq!(config.executor.call_present_evms, 11);
    assert_eq!(config.rpc_server.rpc_address.to_string(), "0.0.0.0:3001");
    assert_eq!(config.miner.block_mode, MinerMode::Interval(std::time::Duration::from_secs(1)));

    // explicit CLI args override file values
    let config = load_with(&["--executor-chain-id", "9999", "-a", "0.0.0.0:3002", "--block-mode", "automine"], file).unwrap();
    assert_eq!(config.executor.executor_chain_id, 9999);
    // file value preserved when not overridden in the CLI
    assert_eq!(config.executor.call_present_evms, 11);
    assert_eq!(config.rpc_server.rpc_address.to_string(), "0.0.0.0:3002");
    assert_eq!(config.miner.block_mode, MinerMode::Automine);
}

#[test]
fn test_clap_defaults_do_not_override_file() {
    // explicitly provided CLI value overrides the file, unrelated defaulted args do not
    let file = r#"
        leader = true

        [executor]
        chain_id = 2008
        call_present_evms = 11

        [common]
        blocking_threads = 64
        async_threads = 4
    "#;
    let config = load_with(&["--async-threads", "8"], file).unwrap();
    assert_eq!(config.common.num_async_threads, 8);
    assert_eq!(config.common.num_blocking_threads, 64);
    assert_eq!(config.executor.call_present_evms, 11);
}

#[test]
fn test_cli_list_arguments_replace_file_values() {
    // a command line list replaces the file's instead of extending it, keeping "the CLI
    // overrides the file" uniform across argument kinds
    let file = r#"
        leader = true

        [executor]
        chain_id = 2008

        [common]
        blocked_clients = ["metamask"]
    "#;
    let config = load_with(&["--blocked-clients", "blockscout"], file).unwrap();
    assert_eq!(config.common.blocked_clients, ["blockscout"]);
}

#[test]
fn test_cli_node_mode_overrides_file() {
    // an explicit CLI node mode is authoritative over the file's, so a config file shared
    // between leader and follower deployments works for both
    let file = r#"
        follower = true

        [executor]
        chain_id = 2008

        [importer]
        external_rpc = "http://localhost:3000/"
    "#;
    let config = load_with(&["--leader"], file).unwrap();
    assert!(config.leader);
    assert!(!config.follower);
    assert!(config.importer.is_none()); // follower section ignored in leader mode

    let file = r#"
        leader = true

        [executor]
        chain_id = 2008

        [importer]
        external_rpc = "http://localhost:3000/"
    "#;
    let config = load_with(&["--follower"], file).unwrap();
    assert!(config.follower);
    assert!(!config.leader);
    assert!(
        config
            .importer
            .as_ref()
            .is_some_and(|importer| importer.external_rpc.as_deref() == Some("http://localhost:3000/"))
    );
}

#[test]
fn test_cli_leader_ignores_cli_importer_args() {
    // #2567: even an explicit CLI importer argument is ignored in leader mode
    let config = load_with(&["--leader", "--executor-chain-id", "2008", "-r", "http://localhost:3000/"], "").unwrap();
    assert!(config.leader);
    assert!(config.importer.is_none());
}

#[test]
fn test_file_only_mode_conflicts_still_fail() {
    // file values are parsed as tokens, so mode exclusivity between file entries is enforced
    // by clap's conflict checks
    let file = r#"
        leader = true
        follower = true

        [executor]
        chain_id = 2008
    "#;
    let error = load_with(&[], file).unwrap_err();
    assert!(error.to_string().contains("cannot be used with"), "unexpected error: {error:#}");
}

#[test]
fn test_zero_chain_id_fails() {
    // zero was the "missing" sentinel before the argument became required
    let file = r#"
        leader = true

        [executor]
        chain_id = 0
    "#;
    let error = load_with(&[], file).unwrap_err();
    assert!(error.to_string().contains("chain id cannot be zero"), "unexpected error: {error:#}");
}

#[test]
fn test_importer_cli_overrides_file() {
    let file = r#"
        follower = true

        [executor]
        chain_id = 2008

        [importer]
        external_rpc = "http://localhost:3000/"
        sync_interval = "250ms"
    "#;

    // partial CLI override of an [importer] section
    let config = load_with(&["-r", "http://localhost:9999/"], file).unwrap();
    let importer = config.importer.as_ref().unwrap();
    assert_eq!(importer.external_rpc.as_deref(), Some("http://localhost:9999/"));
    assert_eq!(importer.sync_interval, std::time::Duration::from_millis(250));

    // importer section only from CLI when file has none
    let file = r#"
        follower = true

        [executor]
        chain_id = 2008
    "#;
    let config = load_with(&["-r", "http://localhost:3000/", "--sync-interval", "1s"], file).unwrap();
    let importer = config.importer.as_ref().unwrap();
    assert_eq!(importer.external_rpc.as_deref(), Some("http://localhost:3000/"));
    assert_eq!(importer.sync_interval, std::time::Duration::from_secs(1));
}

#[test]
fn test_partial_importer_and_kafka_overrides() {
    // a single importer/kafka flag must not demand sibling arguments the file already provides
    let file = r#"
        follower = true

        [executor]
        chain_id = 2008

        [importer]
        external_rpc = "http://127.0.0.1:3000/"
        external_rpc_ws = "ws://127.0.0.1:3000/"
        sync_interval = "5s"

        [kafka]
        bootstrap_servers = "broker:29092"
        topic = "file-topic"
        client_id = "file-client"
    "#;
    let config = load_with(&["--sync-interval", "7ms", "--kafka-topic", "cli-topic"], file).unwrap();

    let importer = config.importer.as_ref().unwrap();
    assert_eq!(importer.external_rpc.as_deref(), Some("http://127.0.0.1:3000/")); // from the file
    assert_eq!(importer.sync_interval, std::time::Duration::from_millis(7)); // from the CLI
    let kafka = config.kafka_config.as_ref().unwrap();
    assert_eq!(kafka.topic.as_deref(), Some("cli-topic")); // from the CLI
    assert_eq!(kafka.bootstrap_servers.as_deref(), Some("broker:29092")); // from the file
    assert_eq!(kafka.client_id.as_deref(), Some("file-client")); // from the file
}

#[test]
fn test_environment_from_cli_and_file() {
    let file = r#"
        leader = true

        [executor]
        chain_id = 2008

        [common]
        env = "canary"
    "#;
    let config = load_with(&[], file).unwrap();
    assert_eq!(config.common.env, Environment::Canary);

    // explicit CLI --env wins over the file
    let config = load_with(&["--env", "staging"], file).unwrap();
    assert_eq!(config.common.env, Environment::Staging);
}

#[test]
fn test_empty_arrays_and_lists() {
    // empty arrays in the file keep the argument at its empty default instead of injecting
    // an empty list value
    let file = r#"
        leader = true

        [executor]
        chain_id = 2008

        [common]
        blocked_clients = []

        [common.tracing]
        headers = []
    "#;
    let config = load_with(&[], file).unwrap();
    assert!(config.common.blocked_clients.is_empty());
    assert!(config.common.tracing.tracing_headers.is_empty());
}
