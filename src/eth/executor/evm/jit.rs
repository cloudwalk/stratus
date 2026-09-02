//! revmc JIT/AOT integration for the executor.
//!
//! When the `revmc` feature is enabled and `--executor-jit` is turned on, contract bytecode is
//! compiled to native code in background threads after it becomes hot, and executed natively
//! instead of being interpreted. Uncompiled (cold) contracts always fall back to the
//! interpreter, so behavior is unchanged on the first executions.
//!
//! With `--executor-jit-aot`, hot contracts are compiled as shared libraries and persisted to
//! the artifact store directory (`--executor-jit-store-path`), so they survive restarts and are
//! dlopened at startup instead of being recompiled.
//!
//! Compiled execution is never used for inspections (`EvmKind::Inspect`): tracing requires
//! step-level inspector callbacks that compiled code does not produce, so those EVMs always
//! interpret.

use revm::Database;

use crate::eth::executor::EvmKind;
use crate::eth::executor::ExecutorConfig;
use crate::eth::executor::evm::types::ExecutorRevm;
use crate::eth::executor::evm::types::GeneralRevm;

/// Handle to the shared revmc JIT backend.
///
/// Clones share the same background compilation thread and the same resident compiled-code
/// map, so a single handle can be distributed to every EVM worker pool.
#[cfg(feature = "revmc")]
#[derive(Clone)]
pub struct JitHandle {
    backend: revmc::runtime::JitBackend,
}

/// Handle to the shared revmc JIT backend (no-op when the `revmc` feature is disabled).
#[cfg(not(feature = "revmc"))]
#[derive(Clone, Default)]
pub struct JitHandle;

#[cfg(feature = "revmc")]
impl JitHandle {
    /// Creates the shared JIT backend from the executor configuration.
    pub fn create(config: &ExecutorConfig) -> Self {
        if !config.executor_jit {
            return Self {
                backend: revmc::runtime::JitBackend::disabled(),
            };
        }

        let mut runtime = revmc::runtime::RuntimeConfig {
            enabled: true,
            aot: config.executor_jit_aot,
            on_compilation: Some(std::sync::Arc::new(|event| {
                tracing::info!(
                    code_hash = ?event.code_hash,
                    spec = ?event.spec_id,
                    duration_ms = event.duration.as_millis() as u64,
                    kind = ?event.kind,
                    success = event.success,
                    "revmc compilation finished"
                );
            })),
            ..Default::default()
        };
        if let Some(threshold) = config.executor_jit_hot_threshold {
            runtime.tuning.jit_hot_threshold = threshold;
        }

        // AOT mode without a working store discards every compilation, so only enable it when
        // the store directory can actually be created.
        if config.executor_jit_aot {
            match FileArtifactStore::new(config.executor_jit_store_path.clone()) {
                Ok(store) => runtime.store = Some(std::sync::Arc::new(store)),
                Err(err) => {
                    tracing::warn!(
                        ?err,
                        path = ?config.executor_jit_store_path,
                        "failed to create revmc AOT artifact store; falling back to in-memory JIT"
                    );
                    runtime.aot = false;
                }
            }
        }

        match revmc::runtime::JitBackend::new(runtime) {
            Ok(backend) => {
                let mode = if config.executor_jit_aot { "aot" } else { "jit" };
                Self::spawn_metrics_reporter(backend.clone(), mode);
                Self { backend }
            }
            Err(err) => {
                tracing::warn!(?err, "failed to create revmc JIT backend; falling back to the interpreter");
                Self {
                    backend: revmc::runtime::JitBackend::disabled(),
                }
            }
        }
    }

    /// Periodically exports revmc runtime statistics as metrics.
    ///
    /// revmc only exposes pull-based counters, so a dedicated thread samples
    /// [`revmc::runtime::JitBackend::stats`] every few seconds and sets gauges. The gauges hold
    /// monotonic counter values (Prometheus `rate()` derives per-second rates from them).
    fn spawn_metrics_reporter(backend: revmc::runtime::JitBackend, mode: &'static str) {
        let spawned = std::thread::Builder::new().name("revmc-metrics".to_string()).spawn(move || {
            use crate::infra::metrics;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
                let stats = backend.stats();
                metrics::set_executor_jit_lookup_hits(stats.lookup_hits, mode);
                metrics::set_executor_jit_lookup_misses(stats.lookup_misses, mode);
                metrics::set_executor_jit_compilations_dispatched(stats.compilations_dispatched, mode);
                metrics::set_executor_jit_compilations_succeeded(stats.compilations_succeeded, mode);
                metrics::set_executor_jit_compilations_failed(stats.compilations_failed, mode);
                metrics::set_executor_jit_compilations_pending(stats.pending_jobs, mode);
                metrics::set_executor_jit_resident_entries(stats.resident_entries, mode);
                metrics::set_executor_jit_evictions(stats.evictions, mode);
                metrics::set_executor_jit_events_dropped(stats.events_dropped, mode);
                metrics::set_executor_jit_code_bytes(stats.jit_code_bytes, mode);
            }
        });
        if spawned.is_err() {
            tracing::warn!("failed to spawn the revmc metrics reporter thread");
        }
    }

    /// Wraps an EVM into JIT-dispatching mode when `kind` allows compiled execution.
    pub fn wrap<DB: Database>(&self, evm: GeneralRevm<DB>, kind: EvmKind) -> ExecutorRevm<DB> {
        if matches!(kind, EvmKind::Inspect) {
            // Inspections must always run the interpreter: tracing needs step-level callbacks
            // that compiled code does not produce.
            return revmc::revm_evm::JitEvm::new(evm, revmc::runtime::JitBackend::disabled());
        }
        revmc::revm_evm::JitEvm::new(evm, self.backend.clone())
    }
}

#[cfg(not(feature = "revmc"))]
#[allow(clippy::unused_self, reason = "mirrors the revmc-enabled signature")]
impl JitHandle {
    /// Creates a no-op handle (the `revmc` feature is disabled).
    pub fn create(_config: &ExecutorConfig) -> Self {
        Self
    }

    /// Returns the EVM unchanged (the `revmc` feature is disabled).
    pub fn wrap<DB: Database>(&self, evm: GeneralRevm<DB>, _kind: EvmKind) -> ExecutorRevm<DB> {
        evm
    }
}

// -----------------------------------------------------------------------------
// AOT artifact store
// -----------------------------------------------------------------------------

/// Filesystem-backed [`ArtifactStore`]: one `.so` plus one `.json` manifest per artifact in a
/// directory.
///
/// Simple benchmark-grade persistence: no locking (revmc only touches the store from its
/// single backend thread), no content verification, and corrupt manifests are skipped with a
/// warning on load.
#[cfg(feature = "revmc")]
#[derive(Debug)]
struct FileArtifactStore {
    /// Directory holding the artifacts.
    dir: std::path::PathBuf,
}

/// Serialized form of [`revmc::runtime::ArtifactManifest`], kept as a `.json` sidecar next to
/// each `.so` artifact.
#[cfg(feature = "revmc")]
#[derive(serde::Serialize, serde::Deserialize)]
struct ManifestDto {
    /// Code hash of the compiled bytecode, hex-encoded without prefix.
    code_hash: String,
    /// Spec ID as its numeric discriminant.
    spec_id: u8,
    /// Compiler backend: "llvm" or "auto".
    backend: String,
    /// Optimization level: "none", "less", "default" or "aggressive".
    opt_level: String,
    /// Symbol name to look up in the shared library.
    symbol_name: String,
    /// Length of the original bytecode.
    bytecode_len: usize,
    /// Length of the compiled artifact in bytes.
    artifact_len: usize,
    /// Creation timestamp (unix seconds).
    created_at_unix_secs: u64,
    /// Keccak-256 digest of the artifact bytes, hex-encoded without prefix.
    content_hash: String,
}

#[cfg(feature = "revmc")]
impl FileArtifactStore {
    /// Creates the store, creating the directory if needed.
    fn new(dir: std::path::PathBuf) -> revmc::eyre::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// Returns the file stem identifying an artifact, matching revmc's own naming scheme.
    fn stem(key: &revmc::runtime::ArtifactKey) -> String {
        format!("{:x}_{:?}_{:?}_{:?}", key.runtime.code_hash, key.runtime.spec_id, key.backend, key.opt_level)
    }

    /// Returns the path of the shared library for an artifact.
    fn artifact_path(&self, key: &revmc::runtime::ArtifactKey) -> std::path::PathBuf {
        self.dir.join(format!("{}.so", Self::stem(key)))
    }

    /// Returns the path of the manifest sidecar for an artifact.
    fn manifest_path(&self, key: &revmc::runtime::ArtifactKey) -> std::path::PathBuf {
        self.dir.join(format!("{}.json", Self::stem(key)))
    }

    /// Reads and validates a manifest sidecar, returning the artifact it describes.
    fn read_artifact(manifest_path: &std::path::Path) -> revmc::eyre::Result<(revmc::runtime::ArtifactKey, revmc::runtime::StoredArtifact)> {
        let dto: ManifestDto = serde_json::from_str(&std::fs::read_to_string(manifest_path)?)?;
        let (key, manifest) = dto
            .to_artifact()
            .ok_or_else(|| revmc::eyre::eyre!("invalid manifest fields | path={}", manifest_path.display()))?;
        let dylib_path = manifest_path.with_extension("so");
        Ok((key, revmc::runtime::StoredArtifact { manifest, dylib_path }))
    }
}

#[cfg(feature = "revmc")]
impl ManifestDto {
    /// Reconstructs the artifact key and manifest, or `None` if any field is invalid.
    fn to_artifact(&self) -> Option<(revmc::runtime::ArtifactKey, revmc::runtime::ArtifactManifest)> {
        let code_hash = revm::primitives::B256::from(parse_hash(&self.code_hash)?);
        let spec_id = revm::primitives::hardfork::SpecId::try_from_u8(self.spec_id)?;
        let backend = match self.backend.as_str() {
            "llvm" => revmc::runtime::BackendSelection::Llvm,
            "auto" => revmc::runtime::BackendSelection::Auto,
            _ => return None,
        };
        let opt_level = opt_level_from_str(&self.opt_level)?;

        let key = revmc::runtime::ArtifactKey {
            runtime: revmc::runtime::RuntimeCacheKey { code_hash, spec_id },
            backend,
            opt_level,
        };
        let manifest = revmc::runtime::ArtifactManifest {
            artifact_key: key.clone(),
            symbol_name: self.symbol_name.clone(),
            bytecode_len: self.bytecode_len,
            artifact_len: self.artifact_len,
            created_at_unix_secs: self.created_at_unix_secs,
            content_hash: parse_hash(&self.content_hash)?,
        };
        Some((key, manifest))
    }
}

#[cfg(feature = "revmc")]
impl revmc::runtime::ArtifactStore for FileArtifactStore {
    fn load_all(&self) -> revmc::eyre::Result<Vec<(revmc::runtime::ArtifactKey, revmc::runtime::StoredArtifact)>> {
        let mut artifacts = Vec::new();
        for entry in std::fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            match Self::read_artifact(&path) {
                Ok(artifact) => artifacts.push(artifact),
                Err(err) => tracing::warn!(?path, ?err, "skipping corrupt revmc AOT artifact manifest"),
            }
        }
        Ok(artifacts)
    }

    fn load(&self, key: &revmc::runtime::ArtifactKey) -> revmc::eyre::Result<Option<revmc::runtime::StoredArtifact>> {
        let manifest_path = self.manifest_path(key);
        if !manifest_path.exists() {
            return Ok(None);
        }
        Ok(Some(Self::read_artifact(&manifest_path)?.1))
    }

    fn store(&self, key: &revmc::runtime::ArtifactKey, manifest: &revmc::runtime::ArtifactManifest, dylib_bytes: &[u8]) -> revmc::eyre::Result<()> {
        let dto = ManifestDto {
            code_hash: format!("{:x}", key.runtime.code_hash),
            spec_id: u8::from(key.runtime.spec_id),
            backend: match key.backend {
                revmc::runtime::BackendSelection::Llvm => "llvm",
                revmc::runtime::BackendSelection::Auto => "auto",
            }
            .into(),
            opt_level: match key.opt_level {
                revmc::OptimizationLevel::None => "none",
                revmc::OptimizationLevel::Less => "less",
                revmc::OptimizationLevel::Default => "default",
                revmc::OptimizationLevel::Aggressive => "aggressive",
            }
            .into(),
            symbol_name: manifest.symbol_name.clone(),
            bytecode_len: manifest.bytecode_len,
            artifact_len: manifest.artifact_len,
            created_at_unix_secs: manifest.created_at_unix_secs,
            content_hash: const_hex::encode(manifest.content_hash),
        };

        // write the library first: load_all scans manifests, so a missing .so for a present
        // manifest is skipped gracefully at startup instead of producing a dangling artifact
        std::fs::write(self.artifact_path(key), dylib_bytes)?;
        std::fs::write(self.manifest_path(key), serde_json::to_vec(&dto)?)?;
        Ok(())
    }

    fn delete(&self, key: &revmc::runtime::ArtifactKey) -> revmc::eyre::Result<()> {
        let _ = std::fs::remove_file(self.artifact_path(key));
        let _ = std::fs::remove_file(self.manifest_path(key));
        Ok(())
    }

    fn clear(&self) -> revmc::eyre::Result<()> {
        for entry in std::fs::read_dir(&self.dir)? {
            let _ = std::fs::remove_file(entry?.path());
        }
        Ok(())
    }
}

/// Decodes 32 bytes from an unprefixed hex string.
#[cfg(feature = "revmc")]
fn parse_hash(hex: &str) -> Option<[u8; 32]> {
    const_hex::decode(hex).ok().and_then(|bytes| bytes.try_into().ok())
}

/// Parses an [`revmc::OptimizationLevel`] name.
#[cfg(feature = "revmc")]
fn opt_level_from_str(name: &str) -> Option<revmc::OptimizationLevel> {
    match name {
        "none" => Some(revmc::OptimizationLevel::None),
        "less" => Some(revmc::OptimizationLevel::Less),
        "default" => Some(revmc::OptimizationLevel::Default),
        "aggressive" => Some(revmc::OptimizationLevel::Aggressive),
        _ => None,
    }
}
