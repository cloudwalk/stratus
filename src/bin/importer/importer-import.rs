use stratus::init_global_services;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let _ = init_global_services();
    Ok(())
}
