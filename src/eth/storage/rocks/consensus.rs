use anyhow::Result;
use k8s_openapi::api::core::v1::Pod;
use kube::api::Api;
use kube::api::ListParams;
use kube::api::ResourceExt;
use kube::Client;

pub async fn gather_clients() -> Result<()> {
    // Infer the runtime environment and try to create a Kubernetes Client
    let client = Client::try_default().await?;

    println!("searching for pods");

    // Read pods in the configured namespace into the typed interface from k8s-openapi
    let pods: Api<Pod> = Api::default_namespaced(client);
    for p in pods.list(&ListParams::default()).await? {
        println!("found pod {}", p.name_any());
    }
    Ok(())
}
