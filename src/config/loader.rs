//! Configuration file loading and CLI merging.
//!
//! Loading rules:
//!
//! 1. The config file is resolved: `--config <path>` when provided, otherwise `config/{binary}.{env}.toml`.
//! 2. The file is parsed with [`toml`]; any field absent from the file falls back to its default value.
//! 3. CLI arguments explicitly provided in the command line override the corresponding file values.
//!    Arguments that only have clap defaults do not override the file.
//! 4. The merged configuration is validated for invariants that depend on file and CLI values together.

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::anyhow;
use clap::ArgMatches;
use clap::Command;
use clap::CommandFactory;
use clap::FromArgMatches;
use clap::parser::ValueSource;

use crate::config::StratusConfig;
use crate::infra::build_info;

/// Configuration that can be loaded from a config file with CLI overrides.
pub trait ConfigLoad: Sized {
    /// Loads the configuration, aborting the process with a readable error message on failure.
    fn load_config() -> Self;
}

impl ConfigLoad for StratusConfig {
    #[allow(clippy::expect_used)]
    fn load_config() -> Self {
        Self::load().unwrap_or_else(|error| {
            println!("failed to load configuration | reason={error:?}");
            std::process::exit(1);
        })
    }
}

impl StratusConfig {
    /// Loads the configuration: config file as base, explicitly provided CLI arguments as overrides.
    pub fn load() -> anyhow::Result<Self> {
        let command = Self::command();
        let matches = command.clone().get_matches();
        Self::load_from_matches(&command, &matches)
    }

    /// Loads the configuration from already parsed CLI matches, reading the resolved config file from disk.
    pub(crate) fn load_from_matches(command: &Command, matches: &ArgMatches) -> anyhow::Result<Self> {
        // resolve the config file path
        let (config_path, explicit_path) = resolve_config_path(matches);

        // read the config file, falling back to defaults when the default file does not exist
        let file_content = match std::fs::read_to_string(&config_path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if explicit_path {
                    return Err(anyhow!("config file not found | path={}", config_path.display()));
                }
                println!("config file not found, using defaults | path={}", config_path.display());
                String::new()
            }
            Err(e) => {
                return Err(anyhow!(e)).context(format!("failed to read config file | path={}", config_path.display()));
            }
        };

        println!("reading config file | path={}", config_path.display());
        Self::load_from_matches_with_content(command, matches, &file_content)
    }

    /// Loads the configuration from already parsed CLI matches and the given config file content.
    fn load_from_matches_with_content(command: &Command, matches: &ArgMatches, file_content: &str) -> anyhow::Result<Self> {
        // parse the config file
        let mut config: StratusConfig = toml::from_str(file_content).with_context(|| "failed to parse config file".to_string())?;

        // merge explicitly provided CLI arguments over the file values
        let cli = StratusConfig::from_arg_matches(matches)?;
        let explicit = explicit_arg_ids(command, matches);
        config.apply_cli_overrides(&cli, &explicit);

        // validate the merged configuration
        config.validate()?;

        Ok(config)
    }
}

/// Resolves the config file path: `--config <path>` when provided, otherwise `config/{binary}.{env}.toml`.
fn resolve_config_path(matches: &ArgMatches) -> (PathBuf, bool) {
    if let Some(path) = matches.get_one::<String>("config_path") {
        return (PathBuf::from(path), true);
    }

    let env = matches
        .get_one::<crate::config::Environment>("env")
        .copied()
        .unwrap_or(crate::config::Environment::Local);
    let path = PathBuf::from(format!("config/{}.{}.toml", build_info::binary_name(), env));
    (path, false)
}

/// Collects the ids of arguments explicitly provided in the command line.
///
/// Arguments filled from clap defaults are excluded, so only values the user actually typed override the config file.
fn explicit_arg_ids(command: &Command, matches: &ArgMatches) -> HashSet<String> {
    command
        .get_arguments()
        .filter(|arg| matches.value_source(arg.get_id().as_str()) == Some(ValueSource::CommandLine))
        .map(|arg| arg.get_id().as_str().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use clap::Parser;

    use crate::config::Environment;
    use crate::config::StratusConfig;
    use crate::eth::miner::MinerMode;

    /// Parses CLI arguments and merges them over the given config file content.
    fn load_with(args: &[&str], file_content: &str) -> anyhow::Result<StratusConfig> {
        let command = StratusConfig::command();
        let matches = command.clone().try_get_matches_from(std::iter::once("stratus").chain(args.iter().copied()))?;
        StratusConfig::load_from_matches_with_content(&command, &matches, file_content)
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
    fn test_mode_flags_from_cli() {
        // follower flag from CLI + importer from file
        let file = r#"
            [executor]
            chain_id = 2008

            [importer]
            external_rpc = "http://localhost:3000/"
        "#;
        let config = load_with(&["--follower"], file).unwrap();
        assert!(config.follower);
        assert_eq!(config.importer.as_ref().unwrap().external_rpc, "http://localhost:3000/");
        config.validate().unwrap();
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
        assert_eq!(importer.external_rpc, "http://localhost:9999/");
        assert_eq!(importer.sync_interval, std::time::Duration::from_millis(250));

        // importer section only from CLI when file has none
        let file = r#"
            follower = true

            [executor]
            chain_id = 2008
        "#;
        let config = load_with(&["-r", "http://localhost:3000/", "--sync-interval", "1s"], file).unwrap();
        let importer = config.importer.as_ref().unwrap();
        assert_eq!(importer.external_rpc, "http://localhost:3000/");
        assert_eq!(importer.sync_interval, std::time::Duration::from_secs(1));
        config.validate().unwrap();
    }

    #[test]
    fn test_clap_defaults_equal_serde_defaults() {
        // clap-parsed defaults must match the serde defaults used when the file omits a field,
        // otherwise a defaulted CLI arg could silently drift from a file default.
        let clap_defaults = StratusConfig::parse_from(["stratus"]);
        let serde_defaults: StratusConfig = toml::from_str("").unwrap();
        assert_eq!(serde_json::to_value(&clap_defaults).unwrap(), serde_json::to_value(&serde_defaults).unwrap());
    }

    #[test]
    fn test_env_arg_selects_file_path() {
        let command = StratusConfig::command();
        let matches = command.clone().try_get_matches_from(["stratus", "--env", "production"]).unwrap();
        let (path, explicit) = super::resolve_config_path(&matches);
        assert!(!explicit);
        assert_eq!(
            path,
            std::path::PathBuf::from(format!("config/{}.production.toml", crate::infra::build_info::binary_name()))
        );

        let matches = command.try_get_matches_from(["stratus", "--config", "/etc/stratus.toml"]).unwrap();
        let (path, explicit) = super::resolve_config_path(&matches);
        assert!(explicit);
        assert_eq!(path, std::path::PathBuf::from("/etc/stratus.toml"));
    }

    #[test]
    fn test_repo_config_files_parse() {
        for path in ["config/stratus.example.toml", "config/stratus.local.toml", "config/stratus-follower.toml"] {
            let content = std::fs::read_to_string(path).unwrap();
            toml::from_str::<StratusConfig>(&content).unwrap_or_else(|e| panic!("failed to parse {path}: {e}"));
        }
    }

    #[test]
    fn test_all_arguments_are_mergeable() {
        // Drift protection: with every argument explicitly provided, the merged result must equal
        // a pure clap parse of the same arguments. A field left uncovered by the `CliOverrides`
        // derive keeps the file value instead, making the two configurations differ.
        //
        // The file flips the default-true boolean flags to false, because setting such flags
        // in the command line produces the same value as their default and cannot detect drift.
        let file = r#"
            follower = true

            [common]
            unknown_client_enabled = false

            [executor]
            chain_id = 421
            reject_not_contract = false

            [importer]
            forward_access_list = false
        "#;
        #[cfg_attr(not(feature = "dev"), allow(unused_mut))]
        let mut args: Vec<&str> = vec![
            "--follower",
            // common
            "--env",
            "staging",
            "--async-threads",
            "7",
            "--blocking-threads",
            "77",
            "--unknown-client-enabled",
            "--blocked-clients",
            "blockscout,metamask",
            // tracing
            "--tracing-url",
            "http://collector:4317",
            "--tracing-protocol",
            "http-json",
            "--tracing-headers",
            "a=b,c=d",
            "--tracing-log-format",
            "json",
            "--tracing-filter",
            "trace",
            // sentry
            "--sentry-url",
            "http://sentry:1234",
            // metrics
            "--metrics-exporter-address",
            "0.0.0.0:9009",
            // rpc
            "--address",
            "0.0.0.0:1234",
            "--max-connections",
            "123",
            "--max-response-size-bytes",
            "1048576",
            "--max-subscriptions",
            "12",
            "--health-check-interval",
            "321",
            "--batch-request-limit",
            "98",
            "--rpc-debug-trace-unsuccessful-only",
            "blockscout",
            // executor
            "--executor-chain-id",
            "421",
            "--executor-call-present-evms",
            "1",
            "--executor-call-past-evms",
            "2",
            "--executor-inspector-evms",
            "3",
            "--executor-reject-not-contract",
            "--executor-evm-spec",
            "Cancun",
            // miner
            "--block-mode",
            "5s",
            // storage.permanent
            "--rocks-path-prefix",
            "sentinel",
            "--rocks-shutdown-timeout",
            "5m",
            "--rocks-disable-sync-write",
            "--rocks-cf-size-metrics-interval",
            "9s",
            "--rocks-file-descriptors-limit",
            "7777",
            // storage.permanent.cf_cache
            "--rocks-cf-cache-accounts",
            "11",
            "--rocks-cf-cache-accounts-history",
            "22",
            "--rocks-cf-cache-account-slots",
            "33",
            "--rocks-cf-cache-account-slots-history",
            "44",
            "--rocks-cf-cache-transactions",
            "55",
            "--rocks-cf-cache-blocks-by-number",
            "66",
            "--rocks-cf-cache-blocks-by-hash",
            "77",
            "--rocks-cf-cache-blocks-by-timestamp",
            "88",
            "--rocks-cf-cache-block-changes",
            "99",
            // storage.cache
            "--account-history-cache-capacity",
            "111",
            "--slot-history-cache-capacity",
            "222",
            // importer
            "--external-rpc",
            "http://localhost:9999/",
            "--external-rpc-ws",
            "ws://localhost:9999/",
            "--external-rpc-timeout",
            "3s",
            "--sync-interval",
            "7ms",
            "--enable-block-changes-replication",
            "--forward-access-list",
            "--stop-at-block",
            "0x2a",
            // kafka
            "--kafka-bootstrap-servers",
            "localhost:29092",
            "--kafka-topic",
            "stratus-events",
            "--kafka-client-id",
            "stratus-producer",
            "--kafka-group-id",
            "stratus-group",
            "--kafka-security-protocol",
            "ssl",
            "--kafka-sasl-mechanisms",
            "plain",
            "--kafka-sasl-username",
            "user",
            "--kafka-sasl-password",
            "pass",
            "--kafka-ssl-ca-location",
            "/ca.pem",
            "--kafka-ssl-certificate-location",
            "/cert.pem",
            "--kafka-ssl-key-location",
            "/key.pem",
        ];
        #[cfg(feature = "dev")]
        args.extend(["--genesis-path", "config/genesis.local.json"]);

        let merged = load_with(&args, file).unwrap();
        let clap_parsed = StratusConfig::parse_from(std::iter::once("stratus").chain(args.iter().copied()));
        assert_eq!(serde_json::to_value(&merged).unwrap(), serde_json::to_value(&clap_parsed).unwrap());
    }

    #[test]
    fn test_config_file_missing_error_only_for_explicit_path() {
        let command = StratusConfig::command();
        let matches = command
            .clone()
            .try_get_matches_from(["stratus", "--config", "/nonexistent/stratus.toml"])
            .unwrap();
        let error = StratusConfig::load_from_matches(&command, &matches).unwrap_err();
        assert!(error.to_string().contains("config file not found"), "unexpected error: {error}");
    }

    #[test]
    fn test_validate_after_merge() {
        // file sets leader, CLI sets follower: clap does not see the conflict, validation must
        let file = r#"
            leader = true

            [executor]
            chain_id = 2008
        "#;
        let error = load_with(&["--follower"], file).unwrap_err();
        assert!(error.to_string().contains("multiple node modes"), "unexpected error: {error}");
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
}
