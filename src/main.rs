use damas::config::parse_config;
use damas::server::Server;

#[compio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("starting server");

    let config = match parse_config("./config.kdl") {
        Ok(c) => {
            tracing::info!(
                port = c.server.listen,
                name = c.server.server_name,
                locations = c.server.locations.len(),
                "🔮 parsed config.kdl and initialized server components"
            );
            tracing::debug!(
                buffer_size = c.server.file_read_buffer_size,
                error_pages = c.server.error_pages.len(),
                max_headers = c.server.max_header_count,
                "detailed configuration loaded"
            );
            c
        }
        Err(report) => {
            tracing::error!(
                error = ?report,
                "💥 critical failure during server startup"
            );
            return Err(anyhow::Error::from_boxed(report.into()));
        }
    };
    Server::from_config(config).await?.run().await
}
