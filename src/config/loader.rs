//! Configuration file loading and CLI merging.
//!
//! Loading rules:
//!
//! 1. The config file is resolved: `--config <path>` when provided, otherwise `config/{binary}.{env}.toml`.
//! 2. The file is parsed with [`toml`] into a table, and each file value becomes a `--flag=value` command
//!    line token for the argument whose `id` matches the value's dotted TOML path. Tokens and arguments
//!    are parsed in a single pass, so the CLI wins over the file; unknown fields are ignored with a warning.
//! 3. A node mode explicitly provided in the command line overrides the file's, and a leader ignores
//!    follower-only sections (`[importer]`, `[kafka]`) instead of failing on them.
//! 4. The merged configuration is validated for invariants that depend on file and CLI values together.
//!
//! Config structs declare the correspondence between file fields and arguments through the argument
//! `id`: each config argument's id is the dotted path of the corresponding TOML field.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::anyhow;
use clap::ArgAction;
use clap::ArgMatches;
use clap::Command;
use clap::CommandFactory;
use clap::Error;
use clap::FromArgMatches;
use clap::Parser;
use clap::error::ErrorKind;
use clap::parser::ValueSource;
use toml::Table;
use toml::Value;

use crate::config::StratusConfig;
use crate::infra::build_info;

/// Arguments that never take values from the config file.
const CLI_ONLY_ARGUMENTS: &[&str] = &["config_path", "nocapture", "help", "version"];

/// Node mode flags: file mode values are skipped when the CLI provides a mode.
const NODE_MODE_ARGUMENTS: &[&str] = &["leader", "follower", "fake_leader"];

/// Sections ignored when the `dev` feature is not enabled.
#[cfg(not(feature = "dev"))]
const IGNORED_FILE_SECTIONS: &[&str] = &["storage.permanent.genesis"];
#[cfg(feature = "dev")]
const IGNORED_FILE_SECTIONS: &[&str] = &[];

/// Configuration that can be loaded from a config file with CLI overrides.
pub trait ConfigLoad: Sized {
    /// Loads the configuration, aborting the process with a readable error message on failure.
    fn load_config() -> Self;
}

impl ConfigLoad for StratusConfig {
    fn load_config() -> Self {
        Self::load().unwrap_or_else(|error| {
            println!("failed to load configuration | reason={error:?}");
            std::process::exit(1);
        })
    }
}

/// Command-line entrypoint: `--config` resolves the file before any configuration value can exist.
#[derive(Parser)]
#[command(author, version, about = "Stratus: EVM executor and JSON-RPC server", long_about = None)]
struct ConfigCli {
    /// Path to the TOML configuration file. When absent, `config/{binary}.{env}.toml` is used.
    #[arg(long = "config", value_name = "FILE")]
    config_path: Option<PathBuf>,

    #[command(flatten)]
    config: StratusConfig,
}

impl ConfigCli {
    /// Resolves the config file path: `--config <path>` when provided, otherwise `config/{binary}.{env}.toml`.
    fn resolve_config_path(&self) -> anyhow::Result<PathBuf> {
        match &self.config_path {
            Some(path) if !path.exists() => Err(anyhow!("config file not found | path={}", path.display())),
            Some(path) => Ok(path.clone()),
            None => Ok(PathBuf::from(format!("config/{}.{}.toml", build_info::binary_name(), self.config.common.env))),
        }
    }
}

impl StratusConfig {
    /// Loads the configuration: config file as base, explicitly provided CLI arguments as overrides.
    pub fn load() -> anyhow::Result<Self> {
        // first pass: resolves the config file path before the file can contribute values
        let first_pass = command_without_mode_requirement().get_matches();
        let cli = ConfigCli::from_arg_matches(&first_pass)?;
        let config_path = cli.resolve_config_path()?;

        // read the config file, falling back to defaults when it does not exist
        let file_content = match std::fs::read_to_string(&config_path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("config file not found, using defaults | path={}", config_path.display());
                String::new()
            }
            Err(e) => {
                return Err(anyhow!(e)).context(format!("failed to read config file | path={}", config_path.display()));
            }
        };

        // parse the file and validate its fields
        println!("reading config file | path={}", config_path.display());
        let table = parse_config_table(&file_content)?;
        let command = ConfigCli::command();
        for field in unknown_fields(&table, &command) {
            println!("warning: unknown field in config file, ignored | field={field}");
        }

        // single parse over the file tokens and the command line: the CLI wins over the file
        let tokens = file_config_tokens(&command, &table, &first_pass);
        let matches = command
            .args_override_self(true)
            .no_binary_name(true)
            .try_get_matches_from(tokens.into_iter().chain(std::env::args_os().skip(1)))
            .map_err(merged_parse_error)?;

        config_from_matches(&matches)
    }
}

/// Maps a missing node mode to a friendlier message that mentions the config file; other errors already describe themselves.
fn merged_parse_error(error: Error) -> anyhow::Error {
    match error.kind() {
        ErrorKind::MissingRequiredArgument => {
            anyhow!("no node mode configured: set exactly one of `leader`, `follower` or `fake_leader` (config file or CLI flag)")
        }
        _ => error.into(),
    }
}

/// The first-pass command: the node mode requirement is relaxed because the mode may come from the config file.
fn command_without_mode_requirement() -> Command {
    ConfigCli::command().mut_group("mode", |group| group.required(false))
}

/// Builds the merged configuration from matches parsed over the file tokens plus the command line.
fn config_from_matches(matches: &ArgMatches) -> anyhow::Result<StratusConfig> {
    let mut config = ConfigCli::from_arg_matches(matches)?.config;

    // a leader ignores follower-only sections instead of failing on them
    config.ignore_follower_sections();
    config.ignore_sentry_without_url();

    // validate the merged configuration
    config.validate()?;
    Ok(config)
}

/// Parses the config file content into a TOML table.
fn parse_config_table(file_content: &str) -> anyhow::Result<Table> {
    toml::from_str(file_content).context("failed to parse config file")
}

/// Converts config file values into `--flag=value` command line tokens; arguments the CLI already provides
/// are skipped, keeping "the CLI overrides the file" uniform.
fn file_config_tokens(command: &Command, table: &Table, cli_matches: &ArgMatches) -> Vec<OsString> {
    let cli_provides = |id: &str| cli_matches.value_source(id) == Some(ValueSource::CommandLine);
    let cli_node_mode = NODE_MODE_ARGUMENTS.iter().any(|mode| cli_provides(mode));
    command
        .get_arguments()
        .filter(|arg| !CLI_ONLY_ARGUMENTS.contains(&arg.get_id().as_str()))
        .filter(|arg| !(cli_node_mode && NODE_MODE_ARGUMENTS.contains(&arg.get_id().as_str())))
        .filter(|arg| !(matches!(arg.get_action(), ArgAction::Append) && cli_provides(arg.get_id().as_str())))
        .filter_map(|arg| {
            let value = table_lookup(table, arg.get_id().as_str()).and_then(toml_value_as_string)?;
            let flag = format!("--{}", arg.get_long()?);
            let token = match arg.get_action() {
                ArgAction::Set | ArgAction::Append => format!("{flag}={value}"),
                _ if value == "true" => flag,
                _ => return None,
            };
            Some(OsString::from(token))
        })
        .collect()
}

/// Collects the paths of fields unknown to the configuration.
fn unknown_fields(table: &Table, command: &Command) -> Vec<String> {
    let known: HashSet<String> = command
        .get_arguments()
        .map(|arg| arg.get_id().as_str().to_string())
        .filter(|id| !CLI_ONLY_ARGUMENTS.contains(&id.as_str()))
        .collect();
    unknown_field_paths(table, "", &known)
}

/// Recursively collects unknown paths.
fn unknown_field_paths(table: &Table, prefix: &str, known: &HashSet<String>) -> Vec<String> {
    let mut unknown = Vec::new();
    for (key, value) in table {
        let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
        if IGNORED_FILE_SECTIONS.contains(&path.as_str()) {
            continue;
        }
        let is_leaf = known.contains(&path);
        let is_section = known.iter().any(|id| id.starts_with(&format!("{path}.")));
        if is_leaf || is_section {
            if let Some(inner) = value.as_table() {
                unknown.extend(unknown_field_paths(inner, &path, known));
            }
        } else {
            unknown.push(path);
        }
    }
    unknown
}

/// Looks up a dotted path in a TOML table, e.g. `storage.permanent.path_prefix`.
fn table_lookup<'a>(table: &'a Table, path: &str) -> Option<&'a Value> {
    let segments: Vec<&str> = path.split('.').collect();
    let (last, parents) = segments.split_last()?;
    let mut current = table;
    for segment in parents {
        current = current.get(*segment)?.as_table()?;
    }
    current.get(*last)
}

/// Converts a TOML value to the string form clap expects for an argument value.
fn toml_value_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(string) => Some(string.clone()),
        Value::Integer(integer) => Some(integer.to_string()),
        Value::Float(float) => Some(float.to_string()),
        Value::Boolean(boolean) => Some(boolean.to_string()),
        Value::Array(items) if items.is_empty() => None,
        Value::Array(items) => Some(items.iter().map(toml_value_as_string).collect::<Option<Vec<_>>>()?.join(",")),
        Value::Datetime(_) | Value::Table(_) => None, // not used by any config argument
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use clap::CommandFactory;
    use clap::FromArgMatches;

    use crate::config::Environment;
    use crate::config::StratusConfig;
    use crate::eth::miner::MinerMode;

    /// Parses CLI arguments over the given config file content and builds the merged configuration.
    fn load_with(args: &[&str], file_content: &str) -> anyhow::Result<StratusConfig> {
        let first_pass = super::command_without_mode_requirement().try_get_matches_from(std::iter::once("stratus").chain(args.iter().copied()))?;
        let table = super::parse_config_table(file_content)?;
        let command = super::ConfigCli::command();
        let tokens = super::file_config_tokens(&command, &table, &first_pass);
        let matches = command
            .args_override_self(true)
            .no_binary_name(true)
            .try_get_matches_from(tokens.into_iter().chain(args.iter().map(|arg| OsString::from(*arg))))
            .map_err(super::merged_parse_error)?;
        super::config_from_matches(&matches)
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
    fn test_every_config_argument_has_a_long_flag() {
        // file values become `--long=value` tokens, so an argument without a long flag would never
        // receive its value from the config file
        let command = super::ConfigCli::command();
        let missing: Vec<&str> = command
            .get_arguments()
            .filter(|arg| !super::CLI_ONLY_ARGUMENTS.contains(&arg.get_id().as_str()))
            .filter(|arg| arg.get_long().is_none())
            .map(|arg| arg.get_id().as_str())
            .collect();
        assert!(missing.is_empty(), "arguments without a long flag: {missing:?}");
    }

    #[test]
    fn test_unknown_fields_are_ignored() {
        // fields unknown to the configuration are ignored (with a warning) instead of failing,
        // so a file with stale options does not prevent startup
        let file = r#"
            leader = true
            misspelled_field = "typo"

            [executor]
            chain_id = 2008
            incorect_name = 3
        "#;
        let command = super::ConfigCli::command();
        let table = super::parse_config_table(file).unwrap();
        let mut unknown = super::unknown_fields(&table, &command);
        unknown.sort();
        assert_eq!(unknown, ["executor.incorect_name", "misspelled_field"]);

        let config = load_with(&[], file).unwrap();
        assert!(config.leader);
        assert_eq!(config.executor.executor_chain_id, 2008);
    }

    #[test]
    fn test_file_values_are_validated_by_clap() {
        // file values go through the same value parsers as the command line, so invalid values
        // fail during the parse
        let file = r#"
            leader = true

            [miner]
            block_mode = "not-a-mode"
        "#;
        let error = load_with(&[], file).unwrap_err();
        assert!(error.to_string().contains("invalid value"), "unexpected error: {error:#}");

        let file = r#"
            leader = true

            [rpc]
            max_response_size_bytes = 300
        "#;
        let error = load_with(&[], file).unwrap_err();
        assert!(error.to_string().contains("must be at least"), "unexpected error: {error:#}");

        // invalid tracing directives are rejected by the value parser instead of being silently
        // dropped by `EnvFilter`
        let file = r#"
            leader = true

            [common.tracing]
            filter = "!!!not-a-directive"
        "#;
        let error = load_with(&[], file).unwrap_err();
        assert!(error.to_string().contains("invalid tracing filter"), "unexpected error: {error:#}");
    }

    #[test]
    fn test_leader_ignores_follower_sections() {
        // #2567: a leader ignores follower-only sections instead of failing on them; the
        // incomplete [kafka] would fail validation for a follower
        let file = r#"
            leader = true

            [executor]
            chain_id = 2008

            [importer]
            external_rpc = "http://localhost:3000/"

            [kafka]
            topic = "stratus-events"
        "#;
        let config = load_with(&[], file).unwrap();
        assert!(config.leader);
        assert!(config.importer.is_none());
        assert!(config.kafka_config.is_none());
    }

    #[test]
    fn test_empty_sentry_url_is_ignored() {
        // an empty sentry url disables the exporter instead of failing to start
        let file = r#"
            leader = true

            [executor]
            chain_id = 2008

            [common.sentry]
        "#;
        let config = load_with(&[], file).unwrap();
        assert!(config.common.sentry.is_none());

        let file = r#"
            leader = true

            [executor]
            chain_id = 2008

            [common.sentry]
            url = ""
        "#;
        let config = load_with(&[], file).unwrap();
        assert!(config.common.sentry.is_none());

        let file = r#"
            leader = true

            [executor]
            chain_id = 2008
        "#;
        let config = load_with(&["--sentry-url", ""], file).unwrap();
        assert!(config.common.sentry.is_none());
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
                .is_some_and(|importer| importer.external_rpc == "http://localhost:3000/")
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
    fn test_missing_node_mode_fails() {
        // the required mode group guarantees at least one mode; here neither the file nor the CLI
        // provides one, and the error message hints at both sources
        let file = r#"
            [executor]
            chain_id = 2008
        "#;
        let error = load_with(&[], file).unwrap_err();
        assert!(error.to_string().contains("no node mode configured"), "unexpected error: {error:#}");
    }

    #[test]
    fn test_follower_still_requires_importer() {
        let file = r#"
            follower = true

            [executor]
            chain_id = 2008
        "#;
        let error = load_with(&[], file).unwrap_err();
        assert!(error.to_string().contains("require `[importer]`"), "unexpected error: {error:#}");
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
    fn test_env_arg_selects_file_path() {
        let command = super::command_without_mode_requirement();
        let matches = command.clone().try_get_matches_from(["stratus", "--env", "production"]).unwrap();
        let cli = super::ConfigCli::from_arg_matches(&matches).unwrap();
        assert_eq!(
            cli.resolve_config_path().unwrap(),
            std::path::PathBuf::from(format!("config/{}.production.toml", crate::infra::build_info::binary_name()))
        );

        // an explicitly provided path must exist
        let matches = command.clone().try_get_matches_from(["stratus", "--config", "/etc/stratus.toml"]).unwrap();
        let cli = super::ConfigCli::from_arg_matches(&matches).unwrap();
        let error = cli.resolve_config_path().unwrap_err();
        assert!(error.to_string().contains("config file not found | path=/etc/stratus.toml"));

        // an existing explicitly provided path is resolved as is
        let matches = command.try_get_matches_from(["stratus", "--config", "config/stratus.local.toml"]).unwrap();
        let cli = super::ConfigCli::from_arg_matches(&matches).unwrap();
        assert_eq!(cli.resolve_config_path().unwrap(), std::path::PathBuf::from("config/stratus.local.toml"));
    }

    #[test]
    fn test_repo_config_files_parse() {
        // the repository config files must parse and survive being converted into command line
        // tokens, which exercises the same value parsers used for the command line; the node
        // mode requirement is relaxed because the example file does not select one
        for path in ["config/stratus.example.toml", "config/stratus.local.toml", "config/stratus-follower.toml"] {
            let content = std::fs::read_to_string(path).unwrap();
            let table = super::parse_config_table(&content).unwrap_or_else(|e| panic!("failed to parse {path}: {e}"));
            let command = super::command_without_mode_requirement();
            assert!(super::unknown_fields(&table, &command).is_empty(), "unknown fields in {path}");
            let first_pass = command.clone().try_get_matches_from(["stratus"]).unwrap();
            let tokens = super::file_config_tokens(&command, &table, &first_pass);
            command
                .args_override_self(true)
                .no_binary_name(true)
                .try_get_matches_from(tokens)
                .unwrap_or_else(|e| panic!("failed to apply values from {path}: {e}"));
        }
    }

    #[test]
    fn test_empty_config_uses_defaults() {
        // an empty file plus the minimal valid arguments must yield exactly the defaults
        let config = load_with(&["--leader", "--executor-chain-id", "1"], "").unwrap();
        let default = StratusConfig {
            leader: true,
            executor: crate::eth::executor::ExecutorConfig {
                executor_chain_id: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(serde_json::to_value(&config).unwrap(), serde_json::to_value(&default).unwrap());
    }

    #[test]
    fn test_full_config_file_covers_all_arguments() {
        // Drift protection: every config argument's dotted id must match a real TOML path, so a
        // fully populated file produces no unknown-field warnings and every value lands in the
        // configuration. An id that drifts from the file format shows up here, either as an
        // unknown field or as a value that never reaches the configuration.
        let file = r#"
            follower = true

            [common]
            env = "production"
            async_threads = 8
            blocking_threads = 64
            unknown_client_enabled = false
            blocked_clients = ["metamask", "blockscout"]

            [common.tracing]
            url = "http://collector:4317"
            protocol = "http-json"
            headers = ["key=value"]
            log_format = "json"
            filter = "debug"

            [common.sentry]
            url = "https://sentry.io/123"

            [common.metrics]
            exporter_address = "0.0.0.0:9001"

            [rpc]
            address = "0.0.0.0:3001"
            max_connections = 100
            max_response_size_bytes = 20971520
            max_subscriptions = 10
            health_check_interval_ms = 200
            batch_request_limit = 50
            debug_trace_unsuccessful_only = ["blockscout"]

            [executor]
            chain_id = 100
            call_present_evms = 1
            call_past_evms = 2
            inspector_evms = 3
            reject_not_contract = false
            evm_spec = "Cancun"

            [miner]
            block_mode = "1s"

            [storage.cache]
            account_history_cache_capacity = 30000
            slot_history_cache_capacity = 400000

            [storage.permanent]
            path_prefix = "temp_3001"
            shutdown_timeout = "1m"
            disable_sync_write = true
            cf_size_metrics_interval = "30s"
            file_descriptors_limit = 1024

            [storage.permanent.cf_cache]
            accounts = 1000
            accounts_history = 2000
            account_slots = 3000
            account_slots_history = 4000
            transactions = 5000
            blocks_by_number = 6000
            blocks_by_hash = 7000
            blocks_by_timestamp = 8000
            block_changes = 9000

            {GENESIS_SECTION}

            [importer]
            external_rpc = "http://localhost:3000/"
            external_rpc_ws = "ws://localhost:3000/"
            external_rpc_timeout = "5s"
            sync_interval = "250ms"
            enable_block_changes_replication = true
            async_threads = 4
            forward_access_list = false
            stop_at_block = "0x2a"

            [kafka]
            bootstrap_servers = "localhost:29092"
            topic = "stratus-events"
            client_id = "stratus-producer"
            group_id = "stratus-group"
            security_protocol = "sasl-ssl"
            sasl_mechanisms = "plain"
            sasl_username = "user"
            sasl_password = "pass"
            ssl_ca_location = "/ca.pem"
            ssl_certificate_location = "/cert.pem"
            ssl_key_location = "/key.pem"
        "#;

        #[cfg(feature = "dev")]
        const GENESIS_SECTION: &str = "[storage.permanent.genesis]\n            path = \"config/genesis.local.json\"";
        #[cfg(not(feature = "dev"))]
        const GENESIS_SECTION: &str = "";

        let file = file.replace("{GENESIS_SECTION}", GENESIS_SECTION);
        let table = super::parse_config_table(&file).unwrap();

        // no field of the fully populated file may be unknown
        let command = super::ConfigCli::command();
        assert!(super::unknown_fields(&table, &command).is_empty(), "unknown fields in the full config file");

        // every config argument must be covered by the fully populated file, except the mutually
        // exclusive node modes (their ids are still guarded by the unknown-field check above)
        for arg in super::ConfigCli::command().get_arguments() {
            let id = arg.get_id().as_str();
            if super::CLI_ONLY_ARGUMENTS.contains(&id) || super::NODE_MODE_ARGUMENTS.contains(&id) {
                continue;
            }
            assert!(
                super::table_lookup(&table, id).is_some(),
                "the full config file does not cover the argument {id} (dotted-path id drifted from the file format?)"
            );
        }

        // every value must land in the configuration
        let config = load_with(&[], &file).unwrap();
        assert!(!config.leader);
        assert!(config.follower);
        assert_eq!(config.common.env, Environment::Production);
        assert_eq!(config.common.num_async_threads, 8);
        assert_eq!(config.common.num_blocking_threads, 64);
        assert!(!config.common.unknown_client_enabled);
        assert_eq!(config.common.blocked_clients, ["metamask", "blockscout"]);
        assert_eq!(config.common.tracing.tracing_url.as_deref(), Some("http://collector:4317"));
        assert_eq!(config.common.tracing.tracing_headers, ["key=value"]);
        assert_eq!(config.common.tracing.tracing_log_format.to_string(), "json");
        assert_eq!(config.common.tracing.tracing_filter.as_deref(), Some("debug"));
        assert_eq!(config.common.sentry.as_ref().unwrap().sentry_url, "https://sentry.io/123");
        assert_eq!(config.common.metrics.metrics_exporter_address.to_string(), "0.0.0.0:9001");
        assert_eq!(config.rpc_server.rpc_address.to_string(), "0.0.0.0:3001");
        assert_eq!(config.rpc_server.rpc_max_connections, 100);
        assert_eq!(config.rpc_server.rpc_max_response_size_bytes, 20971520);
        assert_eq!(
            config.rpc_server.rpc_debug_trace_unsuccessful_only.as_ref().unwrap().len(),
            1,
            "debug_trace_unsuccessful_only must parse into the client set"
        );
        assert_eq!(config.rpc_server.health_check_interval_ms, 200);
        assert_eq!(config.rpc_server.batch_request_limit, 50);
        assert_eq!(config.executor.executor_chain_id, 100);
        assert_eq!(config.executor.call_present_evms, 1);
        assert_eq!(config.executor.call_past_evms, 2);
        assert_eq!(config.executor.inspector_evms, 3);
        assert!(!config.executor.executor_reject_not_contract);
        assert_eq!(config.executor.executor_evm_spec.to_string(), "Cancun");
        assert_eq!(config.miner.block_mode, MinerMode::Interval(std::time::Duration::from_secs(1)));
        assert_eq!(config.storage.cache.account_history_cache_capacity, 30000);
        assert_eq!(config.storage.cache.slot_history_cache_capacity, 400000);
        assert_eq!(config.storage.perm_storage.rocks_path_prefix.as_deref(), Some("temp_3001"));
        assert_eq!(config.storage.perm_storage.rocks_shutdown_timeout, std::time::Duration::from_secs(60));
        assert!(config.storage.perm_storage.rocks_disable_sync_write);
        assert_eq!(
            config.storage.perm_storage.rocks_cf_size_metrics_interval,
            Some(std::time::Duration::from_secs(30))
        );
        assert_eq!(config.storage.perm_storage.rocks_file_descriptors_limit, 1024);
        assert_eq!(config.storage.perm_storage.rocks_cf_cache.accounts, 1000);
        assert_eq!(config.storage.perm_storage.rocks_cf_cache.accounts_history, 2000);
        assert_eq!(config.storage.perm_storage.rocks_cf_cache.account_slots, 3000);
        assert_eq!(config.storage.perm_storage.rocks_cf_cache.account_slots_history, 4000);
        assert_eq!(config.storage.perm_storage.rocks_cf_cache.transactions, 5000);
        assert_eq!(config.storage.perm_storage.rocks_cf_cache.blocks_by_number, 6000);
        assert_eq!(config.storage.perm_storage.rocks_cf_cache.blocks_by_hash, 7000);
        assert_eq!(config.storage.perm_storage.rocks_cf_cache.blocks_by_timestamp, 8000);
        assert_eq!(config.storage.perm_storage.rocks_cf_cache.block_changes, 9000);
        #[cfg(feature = "dev")]
        assert_eq!(
            config.storage.perm_storage.genesis_file.genesis_path.as_deref(),
            Some("config/genesis.local.json")
        );
        let importer = config.importer.as_ref().unwrap();
        assert_eq!(importer.external_rpc, "http://localhost:3000/");
        assert_eq!(importer.external_rpc_ws.as_deref(), Some("ws://localhost:3000/"));
        assert_eq!(importer.external_rpc_timeout, std::time::Duration::from_secs(5));
        assert_eq!(importer.sync_interval, std::time::Duration::from_millis(250));
        assert!(importer.enable_block_changes_replication);
        assert_eq!(importer.importer_async_threads, 4);
        assert!(!importer.forward_access_list);
        assert_eq!(importer.stop_at_block, Some(crate::eth::types::BlockNumber::from(42u64)));
        let kafka = config.kafka_config.as_ref().unwrap();
        assert_eq!(kafka.bootstrap_servers, "localhost:29092");
        assert_eq!(kafka.topic, "stratus-events");
        assert_eq!(kafka.client_id, "stratus-producer");
        assert_eq!(kafka.group_id.as_deref(), Some("stratus-group"));
        assert_eq!(kafka.security_protocol.to_string(), "sasl_ssl");
        assert_eq!(kafka.sasl_mechanisms.as_deref(), Some("plain"));
        assert_eq!(kafka.sasl_username.as_deref(), Some("user"));
        assert_eq!(kafka.sasl_password.as_deref(), Some("pass"));
        assert_eq!(kafka.ssl_ca_location.as_deref(), Some("/ca.pem"));
        assert_eq!(kafka.ssl_certificate_location.as_deref(), Some("/cert.pem"));
        assert_eq!(kafka.ssl_key_location.as_deref(), Some("/key.pem"));

        // the full example must pass validation
        config.validate().unwrap();
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
        assert_eq!(importer.external_rpc, "http://127.0.0.1:3000/"); // from the file
        assert_eq!(importer.sync_interval, std::time::Duration::from_millis(7)); // from the CLI
        let kafka = config.kafka_config.as_ref().unwrap();
        assert_eq!(kafka.topic, "cli-topic"); // from the CLI
        assert_eq!(kafka.bootstrap_servers, "broker:29092"); // from the file
        assert_eq!(kafka.client_id, "file-client"); // from the file
        config.validate().unwrap();
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
    fn test_file_only_sections_materialize() {
        // sections that exist only in the config file (no CLI argument for them) must still
        // materialize their values instead of being dropped as absent
        let file = r#"
            follower = true

            [executor]
            chain_id = 2008

            [common.sentry]
            url = "https://sentry.io/123"

            [importer]
            external_rpc = "http://localhost:3000/"

            [kafka]
            bootstrap_servers = "localhost:29092"
            topic = "stratus-events"
            client_id = "stratus-producer"
        "#;
        let config = load_with(&[], file).unwrap();
        assert_eq!(config.common.sentry.as_ref().unwrap().sentry_url, "https://sentry.io/123");
        assert_eq!(config.importer.as_ref().unwrap().external_rpc, "http://localhost:3000/");
        assert_eq!(config.kafka_config.as_ref().unwrap().topic, "stratus-events");
        config.validate().unwrap();
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
}
