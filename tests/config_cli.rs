use std::ffi::OsString;

use stratus::config::StratusConfig;

fn load_with(args: &[&str], file_content: &str) -> anyhow::Result<StratusConfig> {
    let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
    StratusConfig::load_from(&argv, file_content)
}

fn assert_config_error(args: &[&str], file_content: &str, expected: &str) {
    let error = load_with(args, file_content).err().unwrap();
    assert!(error.to_string().contains(expected), "expected error containing {expected:?}, got {error:#}");
}

const MINIMAL_LEADER: &str = r#"
    leader = true

    [executor]
    chain_id = 2008
"#;

#[test]
fn test_malformed_toml_fails() {
    assert_config_error(&[], "leader =", "failed to parse config file");
}

#[test]
fn test_unknown_field_does_not_prevent_loading() {
    let config = load_with(
        &[],
        r#"
            leader = true
            unknown_field = 1

            [executor]
            chain_id = 2008
        "#,
    )
    .unwrap();
    assert!(config.leader);
    assert_eq!(config.executor.executor_chain_id, 2008);
}

#[test]
fn test_no_node_mode_fails() {
    assert_config_error(&[], "[executor]\nchain_id = 2008", "required arguments were not provided");
}

#[test]
fn test_missing_chain_id_fails() {
    assert_config_error(&[], "leader = true", "--executor-chain-id");
}

#[test]
fn test_leader_ignores_importer_config() {
    let config = load_with(
        &[],
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [importer]
            external_rpc = "http://127.0.0.1:3000/"
        "#,
    )
    .unwrap();
    assert!(config.leader);
    assert!(config.importer.is_none());
}

#[test]
fn test_leader_ignores_kafka_config() {
    let config = load_with(
        &[],
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [kafka]
            topic = "stratus-events"
        "#,
    )
    .unwrap();
    assert!(config.kafka_config.is_none());
}

#[test]
fn test_incomplete_kafka_fails() {
    let config = load_with(
        &[],
        r#"
            follower = true

            [executor]
            chain_id = 2008

            [importer]
            external_rpc = "http://127.0.0.1:3000/"

            [kafka]
            bootstrap_servers = "localhost:29092"
            client_id = "stratus-producer"
        "#,
    )
    .unwrap();
    let error = config.kafka_config.unwrap().init().err().expect("incomplete Kafka config should fail");
    assert!(error.to_string().contains("incomplete `[kafka]` configuration"));
}

#[test]
fn test_empty_sentry_url_is_ignored() {
    let config = load_with(
        &[],
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [common.sentry]
            url = ""
        "#,
    )
    .unwrap();
    assert!(config.common.sentry.is_none());
}

#[test]
fn test_cli_empty_sentry_url_is_ignored() {
    let config = load_with(&["--sentry-url", ""], MINIMAL_LEADER).unwrap();
    assert!(config.common.sentry.is_none());
}

#[test]
fn test_invalid_tracing_filter_fails() {
    assert_config_error(
        &[],
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [common.tracing]
            filter = "invalid=filter=here"
        "#,
        "invalid tracing filter \"invalid=filter=here\"",
    );
}

#[test]
fn test_invalid_block_mode_fails() {
    assert_config_error(
        &[],
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [miner]
            block_mode = "not-a-mode"
        "#,
        "invalid value",
    );
}

#[test]
fn test_invalid_max_response_size_fails() {
    assert_config_error(
        &[],
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [rpc]
            max_response_size_bytes = 300
        "#,
        "must be at least",
    );
}

#[test]
fn test_cli_leader_flag_overrides_file_follower_mode() {
    let config = load_with(
        &["--leader"],
        r#"
            follower = true

            [executor]
            chain_id = 2008

            [importer]
            external_rpc = "http://127.0.0.1:3000/"
        "#,
    )
    .unwrap();
    assert!(config.leader);
    assert!(!config.follower);
    assert!(config.importer.is_none());
}

#[test]
fn test_follower_without_importer_fails() {
    assert_config_error(&[], "follower = true\n[executor]\nchain_id = 2008", "--external-rpc");
}

#[test]
fn test_follower_with_empty_external_rpc_fails() {
    assert_config_error(
        &[],
        r#"
            follower = true

            [executor]
            chain_id = 2008

            [importer]
            external_rpc = ""
        "#,
        "value cannot be empty",
    );
}

#[test]
fn test_cli_follower_flag_overrides_file_leader_mode() {
    assert_config_error(&["--follower"], MINIMAL_LEADER, "--external-rpc");
}
