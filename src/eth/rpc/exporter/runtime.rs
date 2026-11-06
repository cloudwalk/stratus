use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::runtime::Builder;
use tokio::runtime::Handle;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

/// Configuration for the exporter's dedicated Tokio runtime.
pub struct ExporterRuntimeConfig {
    pub async_threads: usize,
    pub blocking_threads: usize,
}

/// Owns the thread on which the exporter's dedicated Tokio runtime runs.
///
/// The runtime serves no tasks of its own: importer-facing RPC methods are dispatched onto its
/// dedicated blocking pool, isolating follower sync traffic from the main runtime's blocking pool.
///
/// Dropping the handle detaches the thread, just like the dedicated importer runtime.
pub struct ExporterRuntime {
    handle: Handle,
    shutdown: CancellationToken,
    thread: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
}

impl ExporterRuntime {
    /// Starts the dedicated exporter runtime and waits until it is ready to accept blocking work.
    pub async fn start(config: ExporterRuntimeConfig) -> anyhow::Result<Self> {
        let (initialized_tx, initialized_rx) = oneshot::channel::<Result<Handle, String>>();
        let shutdown = CancellationToken::new();
        let runtime_shutdown = shutdown.clone();

        let thread = std::thread::Builder::new()
            .name("exporter-runtime".to_owned())
            .spawn(move || run_exporter_runtime(config, runtime_shutdown, initialized_tx))?;

        match initialized_rx.await {
            Ok(Ok(handle)) => Ok(Self {
                handle,
                shutdown,
                thread: Some(thread),
            }),
            Ok(Err(reason)) => {
                let _ = thread.join();
                anyhow::bail!("failed to initialize dedicated exporter runtime: {reason}")
            }
            Err(_) => {
                let result = thread.join();
                anyhow::bail!("dedicated exporter runtime stopped during initialization: {result:?}")
            }
        }
    }

    /// Handle used to dispatch blocking work onto the exporter runtime.
    pub fn handle(&self) -> &Handle {
        &self.handle
    }

    /// Requests shutdown and asynchronously joins the runtime owner thread.
    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        self.shutdown.cancel();
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || match thread.join() {
            Ok(result) => result,
            Err(panic) => anyhow::bail!("dedicated exporter runtime panicked: {panic:?}"),
        })
        .await?
    }
}

impl Drop for ExporterRuntime {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

fn run_exporter_runtime(
    config: ExporterRuntimeConfig,
    shutdown: CancellationToken,
    initialized_tx: oneshot::Sender<Result<Handle, String>>,
) -> anyhow::Result<()> {
    let async_threads = config.async_threads;
    let runtime = match Builder::new_multi_thread()
        .enable_all()
        .worker_threads(config.async_threads)
        .max_blocking_threads(config.blocking_threads)
        .thread_keep_alive(Duration::from_secs(u64::MAX))
        .thread_name_fn(move || {
            // Same convention as the main runtime: Tokio first creates all async threads, then all
            // blocking threads, so the spawn order identifies the pool.
            static ASYNC_ID: AtomicUsize = AtomicUsize::new(1);
            static BLOCKING_ID: AtomicUsize = AtomicUsize::new(1);

            let async_id = ASYNC_ID.fetch_add(1, Ordering::SeqCst);
            if async_id <= async_threads {
                format!("tokio-exporter-{async_id}")
            } else {
                let blocking_id = BLOCKING_ID.fetch_add(1, Ordering::SeqCst);
                format!("tokio-exporter-blocking-{blocking_id}")
            }
        })
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let reason = format!("failed to build exporter Tokio runtime: {error:#}");
            let _ = initialized_tx.send(Err(reason.clone()));
            anyhow::bail!(reason);
        }
    };

    let handle = runtime.handle().clone();
    runtime.block_on(async move {
        let _ = initialized_tx.send(Ok(handle));
        shutdown.cancelled().await;
    });
    Ok(())
}
