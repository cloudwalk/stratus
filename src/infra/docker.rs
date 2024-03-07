use testcontainers::clients::Cli;
use testcontainers::core::WaitFor;
use testcontainers::Container;
use testcontainers::GenericImage;
use testcontainers::RunnableImage;
use testcontainers_modules::postgres::Postgres as PostgresImage;

#[derive(Default)]
pub struct Docker {
    cli: Cli,
}

impl Docker {
    /// Starts PostgreSQL container for local development.
    #[must_use]
    pub fn start_postgres(&self) -> Container<'_, PostgresImage> {
        tracing::info!("starting postgres container");

        let image = RunnableImage::from(PostgresImage::default().with_user("postgres").with_password("123").with_db_name("stratus"))
            .with_mapped_port((5432, 5432))
            .with_volume(("./static/schema/001-init.sql", "/docker-entrypoint-initdb.d/001-schema.sql"))
            .with_volume(("./static/schema/002-schema-external-rpc.sql", "/docker-entrypoint-initdb.d/002-schema.sql"))
            .with_tag("16.2");

        self.cli.run(image)
    }

    /// Starts Prometheus container for local development.
    #[must_use]
    pub fn start_prometheus(&self) -> Container<'_, GenericImage> {
        tracing::info!("starting prometheus container");

        let prometheus_image = GenericImage::new("prom/prometheus", "v2.50.1").with_wait_for(WaitFor::StdErrMessage {
            message: "Starting rule manager...".to_string(),
        });
        let prometheus_args: Vec<String> = vec![
            "--config.file=/etc/prometheus/prometheus.yaml".into(),
            "--storage.tsdb.path=/prometheus".into(),
            "--log.level=debug".into(),
        ];

        let image = RunnableImage::from((prometheus_image, prometheus_args))
            .with_mapped_port((9090, 9090))
            .with_volume(("./static/prometheus.yaml", "/etc/prometheus/prometheus.yaml"));

        self.cli.run(image)
    }

    /// Returns PostgreSQL container URL connection.
    pub fn postgres_connection_url(&self) -> &'static str {
        "postgres://postgres:123@localhost:5432/stratus"
    }

    /// Returns Prometheus container API URL.
    pub fn prometheus_api_url(&self) -> &'static str {
        "http://localhost:9090/api/v1/query"
    }
}
