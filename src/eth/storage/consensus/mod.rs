pub mod client;
pub mod server;

mod test {
    #[tokio::test(flavor = "multi_thread")]
    async fn test_cluster_operations() -> anyhow::Result<()> {
        tokio::spawn(async move {
            crate::eth::storage::consensus::server::run_server().await.expect("Failed to run server");
        });

        // Ensure the server has time to start up
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let mut client = crate::eth::storage::consensus::client::ManagementClient::connect("http://[::1]:50051".to_string()).await?;

        // Test initializing the cluster
        let init_msg = client.init_cluster(1, "127.0.0.1:8080".to_string()).await?;
        assert_eq!(init_msg, "Cluster initialized (mocked).");

        // Test adding a learner
        let add_msg = client.add_learner(2, "127.0.0.1:8081".to_string()).await?;
        assert_eq!(add_msg, "Learner added (mocked).");

        // Test changing membership
        let change_msg = client.change_membership(vec![1, 2, 3]).await?;
        assert_eq!(change_msg, "Membership changed (mocked).");

        Ok(())
    }
}
