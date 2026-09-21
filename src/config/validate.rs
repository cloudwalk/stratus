//! `--validate-config`: parse-time validation of the configuration.
//!
//! Runs the same loading path as a real node and, instead of starting services, prints the
//! divergences that can be known statically (mirroring checks the node performs when services
//! start), the final configuration rendered as TOML in the config-file dialect, and a verdict.
//! Exits with status 0 when the configuration is valid, 1 when it is not.

use anyhow::Context;

use crate::config::StratusConfig;
use crate::infra::kafka::KafkaSecurityProtocol;

/// Runs the validation: prints warnings, errors, the final configuration and the verdict, then
/// exits without starting the node. Never returns.
pub(crate) fn run(config: &StratusConfig) -> ! {
    let (warnings, errors) = static_checks(config);
    for warning in &warnings {
        println!("warning: {warning}");
    }
    for error in &errors {
        println!("error: {error}");
    }

    println!();
    println!("final configuration:");
    match render(config) {
        Ok(rendered) => println!("{rendered}"),
        Err(e) => {
            println!("failed to render the final configuration | reason={e:?}");
            std::process::exit(1);
        }
    }

    if errors.is_empty() {
        println!("configuration is valid");
        std::process::exit(0);
    }
    println!("configuration is invalid | errors={}", errors.len());
    std::process::exit(1);
}

/// Invariants that depend on file and CLI values together and can be checked without side effects.
fn static_checks(config: &StratusConfig) -> (Vec<String>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    errors.extend(kafka_errors(config));
    warnings.extend(miner_warnings(config));
    warnings.extend(genesis_warnings(config));

    (warnings, errors)
}

/// Mirrors the completeness check of `KafkaConnector::new`, which today only fails after the
/// storage boots: an incomplete `[kafka]` is reported before anything starts.
fn kafka_errors(config: &StratusConfig) -> Vec<String> {
    let Some(kafka) = config.kafka_config.as_ref() else {
        return Vec::new();
    };

    let mut missing: Vec<&str> = Vec::new();
    for (field, value) in [
        ("kafka.bootstrap_servers", &kafka.bootstrap_servers),
        ("kafka.topic", &kafka.topic),
        ("kafka.client_id", &kafka.client_id),
    ] {
        if value.is_none() {
            missing.push(field);
        }
    }

    match kafka.security_protocol {
        KafkaSecurityProtocol::SaslSsl => {
            for (field, value) in [
                ("kafka.sasl_mechanisms", &kafka.sasl_mechanisms),
                ("kafka.sasl_username", &kafka.sasl_username),
                ("kafka.sasl_password", &kafka.sasl_password),
            ] {
                if value.is_none() {
                    missing.push(field);
                }
            }
        }
        KafkaSecurityProtocol::Ssl => {
            for (field, value) in [
                ("kafka.ssl_ca_location", &kafka.ssl_ca_location),
                ("kafka.ssl_certificate_location", &kafka.ssl_certificate_location),
                ("kafka.ssl_key_location", &kafka.ssl_key_location),
            ] {
                if value.is_none() {
                    missing.push(field);
                }
            }
        }
        KafkaSecurityProtocol::None => {}
    }

    if missing.is_empty() {
        return Vec::new();
    }
    let fields = missing.iter().map(|field| format!("`{field}`")).collect::<Vec<_>>().join(", ");
    vec![format!("incomplete `[kafka]` configuration: add {fields}")]
}

/// Mirrors `MinerConfig::init`: a follower's miner only starts as external, any other configured
/// block mode is silently overwritten.
fn miner_warnings(config: &StratusConfig) -> Vec<String> {
    if (config.follower || config.fake_leader) && !config.miner.block_mode.is_external() {
        return vec!["conflicting `miner.block_mode`: a follower's miner can only start as external, the configured value is ignored".to_string()];
    }
    Vec::new()
}

/// A genesis file that does not exist is silently replaced by the default genesis block at runtime.
#[cfg(feature = "dev")]
fn genesis_warnings(config: &StratusConfig) -> Vec<String> {
    match config.storage.perm_storage.genesis_file.genesis_path.as_deref() {
        Some(path) if !std::path::Path::new(path).exists() => {
            vec![format!("missing genesis file, the default genesis block will be used | path={path}")]
        }
        _ => Vec::new(),
    }
}

/// No genesis section outside the `dev` feature.
#[cfg(not(feature = "dev"))]
fn genesis_warnings(_config: &StratusConfig) -> Vec<String> {
    Vec::new()
}

/// Renders the configuration as TOML in the config-file dialect. The counterpart of
/// [`StratusConfig::load_from`]: the rendered document can be re-parsed by it.
pub fn render(config: &StratusConfig) -> anyhow::Result<String> {
    let value = toml::Value::try_from(config).context("failed to serialize the final configuration")?;
    toml::to_string_pretty(&value).context("failed to render the final configuration")
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use crate::config::StratusConfig;

    const FOLLOWER_FILE: &str = r#"
        follower = true

        [executor]
        chain_id = 2008

        [importer]
        external_rpc = "http://127.0.0.1:3000/"
    "#;

    /// Parses CLI arguments over the given config file content and builds the merged configuration.
    fn load_with(args: &[&str], file_content: &str) -> StratusConfig {
        let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
        StratusConfig::load_from(&argv, file_content).expect("failed to load config")
    }

    #[test]
    fn test_kafka_completeness() {
        let file = format!(
            "{FOLLOWER_FILE}\n{}",
            r#"
                [kafka]
                bootstrap_servers = "localhost:29092"
                topic = "stratus-events"
                client_id = "stratus-producer"
            "#
        );
        let config = load_with(&[], &file);
        assert!(super::kafka_errors(&config).is_empty());

        let file = format!(
            "{FOLLOWER_FILE}\n{}",
            r#"
                [kafka]
                bootstrap_servers = "localhost:29092"
            "#
        );
        let config = load_with(&[], &file);
        let errors = super::kafka_errors(&config);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("incomplete `[kafka]` configuration"), "unexpected error: {errors:?}");
        assert!(errors[0].contains("`kafka.topic`"), "missing field not reported: {errors:?}");
        assert!(errors[0].contains("`kafka.client_id`"), "missing field not reported: {errors:?}");

        let file = format!(
            "{FOLLOWER_FILE}\n{}",
            r#"
                [kafka]
                bootstrap_servers = "localhost:29092"
                topic = "stratus-events"
                client_id = "stratus-producer"
                security_protocol = "sasl-ssl"
                sasl_username = "user"
            "#
        );
        let config = load_with(&[], &file);
        let errors = super::kafka_errors(&config);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("`kafka.sasl_mechanisms`"), "missing field not reported: {errors:?}");
        assert!(errors[0].contains("`kafka.sasl_password`"), "missing field not reported: {errors:?}");

        let file = format!(
            "{FOLLOWER_FILE}\n{}",
            r#"
                [kafka]
                bootstrap_servers = "localhost:29092"
                topic = "stratus-events"
                client_id = "stratus-producer"
                security_protocol = "ssl"
            "#
        );
        let config = load_with(&[], &file);
        let errors = super::kafka_errors(&config);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("`kafka.ssl_ca_location`"), "missing field not reported: {errors:?}");
        assert!(errors[0].contains("`kafka.ssl_key_location`"), "missing field not reported: {errors:?}");
    }

    #[test]
    fn test_miner_block_mode_conflict() {
        let file = format!(
            "{FOLLOWER_FILE}\n{}",
            r#"
                [miner]
                block_mode = "1s"
            "#
        );
        let config = load_with(&[], &file);
        let warnings = super::miner_warnings(&config);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("block_mode"), "unexpected warning: {warnings:?}");

        let file = format!(
            "{FOLLOWER_FILE}\n{}",
            r#"
                [miner]
                block_mode = "external"
            "#
        );
        let config = load_with(&[], &file);
        assert!(super::miner_warnings(&config).is_empty());

        let file = format!(
            "{FOLLOWER_FILE}\n{}",
            r#"
                [miner]
                block_mode = "1s"
            "#
        );
        let config = load_with(&["--leader"], &file);
        assert!(super::miner_warnings(&config).is_empty());
    }

    #[cfg(feature = "dev")]
    #[test]
    fn test_missing_genesis_file_warns() {
        let file = format!(
            "{FOLLOWER_FILE}\n{}",
            r#"
                [storage.permanent.genesis]
                path = "/nonexistent/genesis.json"
            "#
        );
        let config = load_with(&[], &file);
        let warnings = super::genesis_warnings(&config);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("missing genesis file"), "unexpected warning: {warnings:?}");
        assert!(warnings[0].contains("/nonexistent/genesis.json"), "unexpected warning: {warnings:?}");
    }
}
