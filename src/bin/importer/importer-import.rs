use stratus::config::ImporterImportConfig;
use stratus::init_global_services;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let _: ImporterImportConfig = init_global_services();
    Ok(())
}
