//! Configuration file loading and CLI merging.
//!
//! Loading rules:
//!
//! 1. The config file is resolved by scanning the command line for `--config`/`--env`, without parsing
//!    the arguments: `--config <path>` when provided, otherwise `config/{binary}.{env}.toml`.
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
use std::ffi::OsStr;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::anyhow;
use clap::Arg;
use clap::ArgAction;
use clap::ArgMatches;
use clap::Command;
use clap::CommandFactory;
use clap::Error;
use clap::FromArgMatches;
use clap::Parser;
use clap::error::ErrorKind;
use toml::Table;
use toml::Value;

use crate::config::StratusConfig;
use crate::infra::build_info;

/// Arguments that make no sense as config file fields; they warn as unknown when present in a file.
const CLI_ONLY_ARGUMENTS: &[&str] = &["config_path", "validate_config", "nocapture", "help", "version"];

/// Node mode flags: file mode values are skipped when the CLI provides a mode.
const NODE_MODE_ARGUMENTS: &[&str] = &["leader", "follower", "fake_leader"];

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

    /// Parses the configuration, prints warnings, the final config and the verdict, then exits
    /// without starting the node.
    #[arg(long = "validate-config")]
    validate_config: bool,

    #[command(flatten)]
    config: StratusConfig,
}

/// Resolves the config file path from the command line: `--config <path>` when provided, otherwise `config/{binary}.{env}.toml`.
fn resolve_config_path(argv: &[OsString]) -> anyhow::Result<PathBuf> {
    match flag_value(argv, "--config") {
        Some(config_path) => {
            let config_path = PathBuf::from(config_path);
            if !config_path.exists() {
                Err(anyhow!("config file not found | path={}", config_path.display()))
            } else {
                Ok(config_path)
            }
        }
        None => {
            let env = flag_value(argv, "--env").unwrap_or_else(|| OsString::from("local"));
            Ok(PathBuf::from(format!("config/{}.{}.toml", build_info::binary_name(), env.to_string_lossy())))
        }
    }
}

/// Returns the last value of a flag in the command line, in `--flag <value>` or `--flag=<value>` form.
fn flag_value(argv: &[OsString], flag: &str) -> Option<OsString> {
    let mut value = None;
    let mut tokens = argv.iter().take_while(|token| token.as_os_str() != "--");
    while let Some(token) = tokens.next() {
        if token.as_encoded_bytes() == flag.as_bytes() {
            // the value is the next token; flag-looking values are left for clap to reject
            if let Some(next) = tokens.next().filter(|next| !next.as_encoded_bytes().starts_with(b"-")) {
                value = Some(next.clone());
            }
        } else if let Some(rest) = token.as_encoded_bytes().strip_prefix(format!("{flag}=").as_bytes()) {
            value = Some(OsStr::from_bytes(rest).to_os_string());
        }
    }
    value
}

impl StratusConfig {
    /// Loads the configuration: config file as base, explicitly provided CLI arguments as overrides.
    pub fn load() -> anyhow::Result<Self> {
        let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
        let config_path = resolve_config_path(&argv)?;

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
        println!("reading config file | path={}", config_path.display());

        Self::load_from(&argv, &file_content)
    }

    /// Loads the configuration from an explicit command line and config file content.
    pub fn load_from(argv: &[OsString], file_content: &str) -> anyhow::Result<Self> {
        // parse the file and warn about unknown fields
        let table = parse_config_table(file_content)?;
        let command = ConfigCli::command();
        for field in unknown_fields(&table, &command) {
            println!("warning: unknown field in config file, ignored | field={field}");
        }

        // single parse over the file tokens and the command line: the CLI wins over the file
        let tokens = file_config_tokens(&command, &table, argv);
        let matches = command
            .args_override_self(true)
            .no_binary_name(true)
            .try_get_matches_from(tokens.into_iter().chain(argv.iter().cloned()))
            .map_err(merged_parse_error)?;

        config_from_matches(&matches)
    }
}

/// Prints help and version requests instead of failing; other errors already describe themselves.
fn merged_parse_error(error: Error) -> anyhow::Error {
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => error.exit(),
        _ => error.into(),
    }
}

/// Builds the merged configuration from matches parsed over the file tokens plus the command line.
fn config_from_matches(matches: &ArgMatches) -> anyhow::Result<StratusConfig> {
    let cli = ConfigCli::from_arg_matches(matches)?;
    let mut config = cli.config;

    // a leader ignores follower-only sections instead of failing on them
    config.ignore_follower_sections();
    config.ignore_sentry_without_url();

    if cli.validate_config {
        config.validate_and_exit();
    }

    Ok(config)
}

/// Parses the config file content into a TOML table.
fn parse_config_table(file_content: &str) -> anyhow::Result<Table> {
    toml::from_str(file_content).context("failed to parse config file")
}

/// Returns whether the command line provides the argument, in any of its flag forms.
fn cli_provides(argv: &[OsString], arg: &Arg) -> bool {
    argv.iter().take_while(|token| token.as_os_str() != "--").any(|token| {
        let bytes = token.as_encoded_bytes();
        let provides = |flag: &str| matches!(bytes.strip_prefix(flag.as_bytes()), Some([]) | Some([b'=', ..]));
        arg.get_long().is_some_and(|long| provides(&format!("--{long}")))
            || arg.get_all_aliases().unwrap_or_default().iter().any(|alias| provides(&format!("--{alias}")))
            || arg.get_short().is_some_and(|short| provides(&format!("-{short}")))
    })
}

/// Converts config file values into `--flag=value` command line tokens; arguments the CLI already provides
/// are skipped, keeping "the CLI overrides the file" uniform.
fn file_config_tokens(command: &Command, table: &Table, argv: &[OsString]) -> Vec<OsString> {
    let node_mode = |arg: &Arg| NODE_MODE_ARGUMENTS.contains(&arg.get_id().as_str());
    let cli_node_mode = command.get_arguments().any(|arg| node_mode(arg) && cli_provides(argv, arg));
    command
        .get_arguments()
        .filter(|arg| !(cli_node_mode && node_mode(arg)))
        .filter(|arg| !(matches!(arg.get_action(), ArgAction::Append) && cli_provides(argv, arg)))
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

    use crate::config::Environment;
    use crate::config::StratusConfig;
    use crate::eth::miner::MinerMode;

    /// Parses CLI arguments over the given config file content and builds the merged configuration.
    fn load_with(args: &[&str], file_content: &str) -> anyhow::Result<StratusConfig> {
        let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
        super::StratusConfig::load_from(&argv, file_content)
    }

    #[test]
    fn test_help_contains_config_option() {
        let help = super::ConfigCli::command().render_long_help().to_string();
        assert!(help.contains("--config"));
    }

    #[test]
    fn test_every_config_argument_has_a_long_flag() {
        // file values become `--long=value` tokens, so an argument without a long flag would never
        // receive its value from the config file
        let command = super::ConfigCli::command();
        let missing: Vec<&str> = command
            .get_arguments()
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
    fn test_validate_config_is_cli_only() {
        let file = r#"
            leader = true
            validate_config = true

            [executor]
            chain_id = 2008
        "#;
        let command = super::ConfigCli::command();
        let table = super::parse_config_table(file).unwrap();
        assert_eq!(super::unknown_fields(&table, &command), ["validate_config"]);
    }

    #[test]
    fn test_final_config_toml_round_trip() {
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
            async_threads = 7
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
        let config = load_with(&[], &file).unwrap();
        let rendered = config.render_as_toml().unwrap();
        let table = super::parse_config_table(&rendered).unwrap();
        let command = super::ConfigCli::command();
        let unknown = super::unknown_fields(&table, &command);
        assert!(
            unknown.is_empty(),
            "unknown fields in the rendered configuration (serde field name drifted from the argument id?): {unknown:?}"
        );

        let reparsed = load_with(&[], &rendered).unwrap();
        assert_eq!(
            serde_json::to_value(&config).unwrap(),
            serde_json::to_value(&reparsed).unwrap(),
            "the rendered configuration does not round-trip"
        );
    }

    #[test]
    fn test_cli_provides() {
        let argv = |args: &[&str]| args.iter().map(OsString::from).collect::<Vec<_>>();
        let command = super::ConfigCli::command();
        let arg = |id: &str| command.get_arguments().find(|arg| arg.get_id().as_str() == id).unwrap();

        // both flag forms count as provided; a longer flag is not this one, and `--` ends the flags
        let blocked_clients = arg("common.blocked_clients");
        assert!(super::cli_provides(&argv(&["--blocked-clients", "a"]), blocked_clients));
        assert!(super::cli_provides(&argv(&["--blocked-clients=a"]), blocked_clients));
        assert!(!super::cli_provides(&argv(&["--blocked-clients-x"]), blocked_clients));
        assert!(!super::cli_provides(&argv(&["--", "--blocked-clients=a"]), blocked_clients));

        // mode flags follow the same presence semantics clap uses for conflicts
        let follower = arg("follower");
        assert!(super::cli_provides(&argv(&["--follower"]), follower));
        assert!(super::cli_provides(&argv(&["--follower=false"]), follower));
        assert!(!super::cli_provides(&argv(&["--follower-x"]), follower));
    }

    #[test]
    fn test_env_arg_selects_file_path() {
        let argv = |args: &[&str]| args.iter().map(OsString::from).collect::<Vec<_>>();
        let default = |env: &str| std::path::PathBuf::from(format!("config/{}.{}.toml", crate::infra::build_info::binary_name(), env));

        // `--env` selects the environment's default file, in both flag forms
        assert_eq!(super::resolve_config_path(&argv(&["--env", "production"])).unwrap(), default("production"));
        assert_eq!(super::resolve_config_path(&argv(&["--env=production"])).unwrap(), default("production"));

        // the last occurrence wins; an explicitly provided path must exist
        let error = super::resolve_config_path(&argv(&["--config=/missing.toml", "--config", "/etc/stratus.toml"])).unwrap_err();
        assert!(error.to_string().contains("config file not found | path=/etc/stratus.toml"));

        // an existing explicitly provided path is resolved as is, in both flag forms
        assert_eq!(
            super::resolve_config_path(&argv(&["--config", "config/stratus.local.toml"])).unwrap(),
            std::path::PathBuf::from("config/stratus.local.toml")
        );
        assert_eq!(
            super::resolve_config_path(&argv(&["--config=config/stratus.local.toml"])).unwrap(),
            std::path::PathBuf::from("config/stratus.local.toml")
        );

        // flag-looking values are left for clap to reject, and `--` ends the flags
        assert_eq!(super::resolve_config_path(&argv(&["--config", "--leader"])).unwrap(), default("local"));
        assert_eq!(
            super::resolve_config_path(&argv(&["--", "--config", "/etc/stratus.toml"])).unwrap(),
            default("local")
        );
    }

    #[test]
    fn test_repo_config_files_parse() {
        // the repository config files must parse and survive being converted into command line
        // tokens, which exercises the same value parsers used for the command line; the node mode
        // comes from the command line, where the deployed binaries always select it
        let files = [
            ("config/stratus.example.toml", "--leader"),
            ("config/stratus.local.toml", "--leader"),
            ("config/stratus-follower.toml", "--follower"),
        ];
        for (path, mode) in files {
            let content = std::fs::read_to_string(path).unwrap();
            let table = super::parse_config_table(&content).unwrap_or_else(|e| panic!("failed to parse {path}: {e}"));
            let command = super::ConfigCli::command();
            assert!(super::unknown_fields(&table, &command).is_empty(), "unknown fields in {path}");
            let argv: Vec<OsString> = vec![OsString::from(mode)];
            let tokens = super::file_config_tokens(&command, &table, &argv);
            command
                .args_override_self(true)
                .no_binary_name(true)
                .try_get_matches_from(tokens.into_iter().chain(argv))
                .unwrap_or_else(|e| panic!("failed to apply values from {path}: {e}"));
        }
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

            [exporter]
            async_threads = 2
            blocking_threads = 32

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

        // a representative value from each section must land in the configuration; the loops above
        // guarantee every argument is covered, so this samples the value kinds (enums, durations,
        // lists, hex numbers, serde renames) instead of asserting every field
        let config = load_with(&[], &file).unwrap();
        assert!(!config.leader);
        assert!(config.follower);
        assert_eq!(config.common.env, Environment::Production);
        assert_eq!(config.common.num_async_threads, 8);
        assert_eq!(config.common.blocked_clients, ["metamask", "blockscout"]);
        assert_eq!(config.common.tracing.tracing_filter.as_deref(), Some("debug"));
        assert_eq!(config.common.sentry.as_ref().unwrap().sentry_url, "https://sentry.io/123");
        assert_eq!(config.common.metrics.metrics_exporter_address.to_string(), "0.0.0.0:9001");
        assert_eq!(config.rpc_server.rpc_address.to_string(), "0.0.0.0:3001");
        assert_eq!(config.rpc_server.rpc_max_response_size_bytes, 20971520);
        assert_eq!(config.executor.executor_chain_id, 100);
        assert_eq!(config.executor.executor_evm_spec.to_string(), "Cancun");
        assert_eq!(config.miner.block_mode, MinerMode::Interval(std::time::Duration::from_secs(1)));
        assert_eq!(config.exporter.exporter_async_threads, 2);
        assert_eq!(config.exporter.exporter_blocking_threads, 32);
        assert_eq!(config.storage.perm_storage.rocks_path_prefix.as_deref(), Some("temp_3001"));
        assert_eq!(config.storage.perm_storage.rocks_file_descriptors_limit, 1024);
        assert_eq!(config.storage.perm_storage.rocks_cf_cache.accounts, 1000);
        #[cfg(feature = "dev")]
        assert_eq!(
            config.storage.perm_storage.genesis_file.genesis_path.as_deref(),
            Some("config/genesis.local.json")
        );
        let importer = config.importer.as_ref().unwrap();
        assert_eq!(importer.external_rpc.as_deref(), Some("http://localhost:3000/"));
        assert_eq!(importer.sync_interval, std::time::Duration::from_millis(250));
        assert_eq!(importer.stop_at_block, Some(crate::eth::types::BlockNumber::from(42u64)));
        let kafka = config.kafka_config.as_ref().unwrap();
        assert_eq!(kafka.bootstrap_servers.as_deref(), Some("localhost:29092"));
        assert_eq!(kafka.group_id.as_deref(), Some("stratus-group"));
        assert_eq!(kafka.security_protocol.to_string(), "sasl_ssl");
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
}
