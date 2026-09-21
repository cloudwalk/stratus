//! `--validate-config`: parse-time validation of the configuration.
//!
//! Runs the same loading path as a real node and, instead of starting services, prints the
//! divergences that can be known statically (mirroring checks the node performs when services
//! start), the final configuration rendered as TOML in the config-file dialect, and a verdict.
//! Exits with status 0 when the configuration is valid, 1 when it is not.

mod palette;

use anyhow::Context;

use self::palette::Palette;
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
pub struct Validation {
    /// Findings that do not fail the validation.
    pub warnings: Vec<String>,

    /// Findings that fail the validation.
    pub errors: Vec<String>,
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
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Prints the findings, warnings first.
    fn print(&self, palette: &Palette) {
        for warning in &self.warnings {
            println!("{} {warning}", palette.warning("warning:"));
        }

        for error in &self.errors {
            println!("{} {error}", palette.error("error:"));
        }
    }
}

impl StratusConfig {
    /// Runs the validation workflow over the final configuration and returns its findings.
    pub fn validate(&self) -> Validation {
        Validation::run(self)
    }

    /// Renders the configuration as TOML in the config-file dialect.
    pub fn render_as_toml(&self) -> anyhow::Result<String> {
        let value = toml::Value::try_from(self).context("failed to serialize the final configuration")?;
        toml::to_string_pretty(&value).context("failed to render the final configuration")
    }

    /// `--validate-config`: prints the validation report and the final configuration, then exits
    /// without starting the node. Never returns.
    pub(crate) fn validate_and_exit(&self) -> ! {
        let palette = Palette::detect();
        let validation = self.validate();
        validation.print(&palette);

        println!();
        println!("{}", palette.heading("final configuration:"));
        println!("{}", palette.rule());
        match self.render_as_toml() {
            Ok(rendered) => println!("{rendered}"),
            Err(e) => {
                println!("failed to render the final configuration | reason={e:?}");
                std::process::exit(1);
            }
        }
        println!("{}", palette.rule());

        if validation.is_valid() {
            println!("{}", palette.valid("configuration is valid"));
            std::process::exit(0);
        }
        let verdict = format!("configuration is invalid | errors={}", validation.errors.len());
        println!("{}", palette.error(&verdict));
        std::process::exit(1);
    }
}
