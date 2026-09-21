//! `--validate-config`: parse-time validation of the configuration.
//!
//! Runs the same loading path as a real node and, instead of starting services, prints the
//! divergences that can be known statically (mirroring checks the node performs when services
//! start), the final configuration rendered as TOML in the config-file dialect, and a verdict.
//! Exits with status 0 when the configuration is valid, 1 when it is not.

use anyhow::Context;

use crate::config::StratusConfig;
use crate::infra::kafka::KafkaSecurityProtocol;

/// Severity of a step's findings: errors fail the validation, warnings do not.
enum Severity {
    Warning,
    Error,
}

/// One workflow step: an independent inspection of the final configuration.
enum Step {
    /// Mirrors the completeness check of `KafkaConnector::new`
    KafkaCompleteness,

    /// Mirrors `MinerConfig::init`
    MinerExternalBlockMode,

    /// A genesis file that does not exist is silently replaced by the default genesis block at runtime.
    #[cfg(feature = "dev")]
    GenesisFileExists,
}

impl Step {
    const WORKFLOW: &'static [Self] = &[
        Self::KafkaCompleteness,
        Self::MinerExternalBlockMode,
        #[cfg(feature = "dev")]
        Self::GenesisFileExists,
    ];

    /// Findings of this severity make the configuration invalid.
    fn severity(&self) -> Severity {
        match self {
            Self::KafkaCompleteness => Severity::Error,
            Self::MinerExternalBlockMode => Severity::Warning,
            #[cfg(feature = "dev")]
            Self::GenesisFileExists => Severity::Warning,
        }
    }

    /// Runs the step and returns its findings.
    fn check(&self, config: &StratusConfig) -> Vec<String> {
        match self {
            Self::KafkaCompleteness => Self::check_kafka_completeness(config),
            Self::MinerExternalBlockMode => Self::check_miner_external_block_mode(config),
            #[cfg(feature = "dev")]
            Self::GenesisFileExists => Self::check_genesis_file_exists(config),
        }
    }

    /// Returns an error listing the `[kafka]` fields required by the configured security protocol
    /// that are missing from the configuration.
    fn check_kafka_completeness(config: &StratusConfig) -> Vec<String> {
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

    /// Returns a warning when a follower's miner block mode is not external.
    fn check_miner_external_block_mode(config: &StratusConfig) -> Vec<String> {
        if (config.follower || config.fake_leader) && !config.miner.block_mode.is_external() {
            return vec!["conflicting `miner.block_mode`: a follower's miner can only start as external, the configured value is ignored".to_string()];
        }
        Vec::new()
    }

    /// Returns a warning when the configured genesis file does not exist.
    #[cfg(feature = "dev")]
    fn check_genesis_file_exists(config: &StratusConfig) -> Vec<String> {
        match config.storage.perm_storage.genesis_file.genesis_path.as_deref() {
            Some(path) if !std::path::Path::new(path).exists() => {
                vec![format!("missing genesis file, the default genesis block will be used | path={path}")]
            }
            _ => Vec::new(),
        }
    }
}

/// The findings of one validation run.
#[derive(Default)]
struct Validation {
    warnings: Vec<String>,
    errors: Vec<String>,
}

impl Validation {
    /// Runs every step in [`Step::WORKFLOW`] over the final configuration.
    fn run(config: &StratusConfig) -> Self {
        let mut validation = Self::default();

        for step in Step::WORKFLOW {
            let findings = step.check(config);
            match step.severity() {
                Severity::Warning => validation.warnings.extend(findings),
                Severity::Error => validation.errors.extend(findings),
            }
        }

        validation
    }

    /// Returns whether the configuration is valid: no errors, warnings allowed.
    fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Prints the findings, warnings first.
    fn print(&self) {
        for warning in &self.warnings {
            println!("warning: {warning}");
        }

        for error in &self.errors {
            println!("error: {error}");
        }
    }
}

impl StratusConfig {
    /// Renders the configuration as TOML in the config-file dialect.
    pub fn render_as_toml(&self) -> anyhow::Result<String> {
        let value = toml::Value::try_from(self).context("failed to serialize the final configuration")?;
        toml::to_string_pretty(&value).context("failed to render the final configuration")
    }

    /// `--validate-config`: prints the validation report and the final configuration, then exits
    /// without starting the node. Never returns.
    pub(crate) fn validate_and_exit(&self) -> ! {
        let validation = Validation::run(self);
        validation.print();

        println!();
        println!("final configuration:");
        match self.render_as_toml() {
            Ok(rendered) => println!("{rendered}"),
            Err(e) => {
                println!("failed to render the final configuration | reason={e:?}");
                std::process::exit(1);
            }
        }

        if validation.is_valid() {
            println!("configuration is valid");
            std::process::exit(0);
        }
        println!("configuration is invalid | errors={}", validation.errors.len());
        std::process::exit(1);
    }
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
        assert!(super::Step::KafkaCompleteness.check(&config).is_empty());

        let file = format!(
            "{FOLLOWER_FILE}\n{}",
            r#"
                [kafka]
                bootstrap_servers = "localhost:29092"
            "#
        );
        let config = load_with(&[], &file);
        let errors = super::Step::KafkaCompleteness.check(&config);
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
        let errors = super::Step::KafkaCompleteness.check(&config);
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
        let errors = super::Step::KafkaCompleteness.check(&config);
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
        let warnings = super::Step::MinerExternalBlockMode.check(&config);
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
        assert!(super::Step::MinerExternalBlockMode.check(&config).is_empty());

        let file = format!(
            "{FOLLOWER_FILE}\n{}",
            r#"
                [miner]
                block_mode = "1s"
            "#
        );
        let config = load_with(&["--leader"], &file);
        assert!(super::Step::MinerExternalBlockMode.check(&config).is_empty());
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
        let warnings = super::Step::GenesisFileExists.check(&config);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("missing genesis file"), "unexpected warning: {warnings:?}");
        assert!(warnings[0].contains("/nonexistent/genesis.json"), "unexpected warning: {warnings:?}");
    }

    #[test]
    fn test_workflow_collects_all_findings() {
        let file = format!(
            "{FOLLOWER_FILE}\n{}",
            r#"
                [miner]
                block_mode = "1s"

                [kafka]
                bootstrap_servers = "localhost:29092"
            "#
        );
        let config = load_with(&[], &file);
        let validation = super::Validation::run(&config);
        assert!(!validation.is_valid());
        assert_eq!(validation.errors.len(), 1, "unexpected errors: {:?}", validation.errors);
        assert_eq!(validation.warnings.len(), 1, "unexpected warnings: {:?}", validation.warnings);

        let file = format!(
            "{FOLLOWER_FILE}\n{}",
            r#"
                [miner]
                block_mode = "external"
            "#
        );
        let config = load_with(&[], &file);
        let validation = super::Validation::run(&config);
        assert!(validation.is_valid());
        assert!(validation.warnings.is_empty(), "unexpected warnings: {:?}", validation.warnings);
        assert!(validation.errors.is_empty(), "unexpected errors: {:?}", validation.errors);
    }
}
