use crate::ServerContext;
use crate::config::Config;
use crate::error::ErrorRegistry;
use crate::http::handle_request;
use crate::index::IndexCache;
use crate::response::error_response;
use crate::router::RouterNode;
use compio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use compio::net::TcpListener;
use compio::runtime::spawn;
use minijinja::Environment;
use once_cell::sync::Lazy;
use tracing::{Instrument, Span, info_span};

static JINJA_ENV: Lazy<Environment<'static>> = Lazy::new(|| {
    let mut env = Environment::new();
    env.add_template("error", include_str!("../template/error.html"))
        .unwrap();
    env.add_template("index", include_str!("../template/index.html"))
        .unwrap();
    env
});

pub struct Server {
    router: RouterNode,
    config: Config,
    error_registry: ErrorRegistry,
}

impl Server {
    pub async fn from_config(config: Config) -> anyhow::Result<Self, anyhow::Error> {
        let router = RouterNode::from_config(&config)?;
        tracing::info!("created router from config");
        let error_registry = ErrorRegistry::new(&JINJA_ENV, 100);
        error_registry.init_with_config(&config).await;
        tracing::info!("initialized error registry");

        Ok(Self {
            router,
            config,
            error_registry,
        })
    }

    pub async fn run(self) -> anyhow::Result<(), anyhow::Error> {
        let Server {
            router,
            config,
            error_registry,
        } = self;
        let index_cache = IndexCache::new(&JINJA_ENV, 100);
        let context = ServerContext::new(config, router, error_registry, index_cache);
        let addr = format!(
            "{}:{}",
            context.config.server.server_name, context.config.server.listen
        );

        let listener = TcpListener::bind(&addr).await.inspect_err(|_e| {
            tracing::error!("Failed to bind to {}", addr);
        })?;

        tracing::info!("🚀 Server started at {}", addr);

        loop {
            match listener.accept().await {
                Ok((stream, address)) => {
                    tracing::info!("Accepted connection from {}", address);
                    let ctx = context.clone();
                    let span = info_span!(
                        "handle_connection",
                        address = %address,
                        method = tracing::field::Empty,
                        path = tracing::field::Empty,
                        status = tracing::field::Empty
                    );
                    spawn(async move { handle_connection(stream, ctx).instrument(span).await })
                        .detach();
                }
                Err(err) => {
                    tracing::error!("Error accepting connection: {}", err);
                }
            }
        }
    }
}

async fn handle_connection<T: AsyncRead + AsyncWrite>(mut stream: T, context: ServerContext) -> () {
    match handle_request(&mut stream, &context)
        .instrument(Span::current())
        .await
    {
        Ok(Ok(())) => {
            tracing::info!("Request handled successfully");
        }
        Ok(Err(expected)) => {
            tracing::error!("Expected error: {}", expected);
        }
        Err(err) => {
            tracing::error!("Error handling request: {}", err);
            let response = error_response(&context.error_registry, 500).await;
            let _ = stream.write_all(response).await;
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::config::*;
    use crate::router::{MatchType, RouterHandler};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn create_mock_config<F>(modifier: F) -> Config
    where
        F: FnOnce(&mut Config),
    {
        let mut config = Config {
            server: ServerConfig {
                listen: 80,
                server_name: "localhost".to_string(),
                locations: vec![
                    LocationConfig {
                        path: Path::new("/").to_path_buf(),
                        root: Path::new("/www/var/html").to_path_buf(),
                        index: vec!["index.html".to_string(), "index.htm".to_string()],
                        ty: Some(LocationConfigType::Prefix),
                        ..Default::default()
                    },
                    LocationConfig {
                        path: Path::new("/50x.html").to_path_buf(),
                        root: Path::new("/www/var/html").to_path_buf(),
                        index: vec![],
                        ty: Some(LocationConfigType::Exact),
                        ..Default::default()
                    },
                ],
                error_pages: vec![ErrorPage {
                    path: Path::new("/40x.html").to_path_buf(),
                    root: Path::new("/var/www/html").to_path_buf(),
                    files: ErrorFiles {
                        codes: vec![
                            ErrorCodeEntry {
                                status: 400,
                                file: Path::new("400.html").to_path_buf(),
                            },
                            ErrorCodeEntry {
                                status: 401,
                                file: Path::new("unauthorized.html").to_path_buf(),
                            },
                            ErrorCodeEntry {
                                status: 402,
                                file: Path::new("402.html").to_path_buf(),
                            },
                            ErrorCodeEntry {
                                status: 404,
                                file: Path::new("forbidden.html").to_path_buf(),
                            },
                        ],
                    },
                }],
                connection_buffer_size: 4096,
                file_read_buffer_size: 8192,
                max_header_count: 64,
            },
        };

        modifier(&mut config);

        config
    }

    #[compio::test]
    async fn test_from_config_routing_registration() -> anyhow::Result<()> {
        let config = create_mock_config(|c| {
            c.server.locations = vec![
                LocationConfig {
                    path: PathBuf::from("/"),
                    ty: Some(LocationConfigType::Prefix),
                    root: PathBuf::from("/www/root"),
                    index: vec![],
                    ..Default::default()
                },
                LocationConfig {
                    path: PathBuf::from("/static"),
                    ty: Some(LocationConfigType::Prefix),
                    root: PathBuf::from("/www/static"),
                    index: vec!["static.html".to_string()],
                    ..Default::default()
                },
                LocationConfig {
                    path: PathBuf::from("/50x.html"),
                    ty: Some(LocationConfigType::Exact),
                    root: PathBuf::from("/www/errors"),
                    index: vec![],
                    ..Default::default()
                },
                LocationConfig {
                    path: PathBuf::from("/static/images"),
                    ty: Some(LocationConfigType::Prefix),
                    root: PathBuf::from("/www/images"),
                    index: vec!["image.jpg".to_string()],
                    ..Default::default()
                },
            ];
        });

        let server = Server::from_config(config).await?;

        let res_img = server.router.search("/static/images/logo.png");
        assert!(res_img.is_some());
        assert_eq!(
            res_img.unwrap().0,
            RouterHandler {
                root: Arc::from("/www/images"),
                matched_path: Arc::from("/static/images"),
                index: Arc::from(vec![String::from("image.jpg")]),
                match_type: MatchType::Prefix,
                is_auto_index: false,
            },
            "Should match the longest prefix (/static/images)"
        );

        let res_static = server.router.search("/static/style.css");
        assert!(res_static.is_some());
        assert_eq!(
            res_static.unwrap().0,
            RouterHandler {
                root: Arc::from("/www/static"),
                matched_path: Arc::from("/static"),
                index: Arc::from(vec![String::from("static.html")]),
                match_type: MatchType::Prefix,
                is_auto_index: false,
            }
        );

        let res_root = server.router.search("/unknown/path");
        assert!(res_root.is_some());

        Ok(())
    }
}
