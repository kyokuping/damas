use crate::config::Config;
use crate::router::RouterNode;
use crate::{ServerContext, handle_connection};
use compio::net::TcpListener;
use compio::runtime::spawn;

pub struct Server {
    router: RouterNode,
    config: Config,
}

impl Server {
    pub fn from_config(config: Config) -> anyhow::Result<Self, anyhow::Error> {
        let router = RouterNode::from_config(&config)?;
        Ok(Self { router, config })
    }

    pub async fn run(self) -> anyhow::Result<(), anyhow::Error> {
        let config: &'static Config = Box::leak(Box::new(self.config));
        let router: &'static RouterNode = Box::leak(Box::new(self.router));
        let host = &config.server.server_name;
        let port = config.server.listen;
        let listener = match TcpListener::bind(format!("{}:{}", host, port)).await {
            Ok(listener) => {
                println!("Server started at {}:{}", host, port);
                listener
            }
            Err(err) => {
                println!("Failed to bind to {}:{}", host, port);
                Err(err)?
            }
        };

        loop {
            match listener.accept().await {
                Ok((stream, address)) => {
                    println!("Accepted connection from {}", address);
                    spawn(async move {
                        if let Err(err) =
                            handle_connection(stream, ServerContext { config, router }).await
                        {
                            eprintln!("Error handling connection: {}", err);
                        }
                    })
                    .detach();
                }
                Err(err) => {
                    eprintln!("Error accepting connection: {}", err);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ErrorPage, LocationConfig, LocationConfigType, ServerConfig};
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
                    },
                    LocationConfig {
                        path: Path::new("/50x.html").to_path_buf(),
                        root: Path::new("/www/var/html").to_path_buf(),
                        index: vec![],
                        ty: Some(LocationConfigType::Exact),
                    },
                ],
                error_pages: vec![ErrorPage {
                    path: Path::new("/50x.html").to_path_buf(),
                    codes: vec![500, 502, 503, 504],
                }],
                connection_buffer_size: 4096,
                file_read_buffer_size: 8192,
                max_header_count: 64,
            },
        };

        modifier(&mut config);

        config
    }

    #[test]
    fn test_from_config_routing_registration() -> anyhow::Result<()> {
        let config = create_mock_config(|c| {
            c.server.locations = vec![
                LocationConfig {
                    path: PathBuf::from("/"),
                    ty: Some(LocationConfigType::Prefix),
                    root: PathBuf::from("/www/root"),
                    index: vec![],
                },
                LocationConfig {
                    path: PathBuf::from("/static"),
                    ty: Some(LocationConfigType::Prefix),
                    root: PathBuf::from("/www/static"),
                    index: vec!["static.html".to_string()],
                },
                LocationConfig {
                    path: PathBuf::from("/50x.html"),
                    ty: Some(LocationConfigType::Exact),
                    root: PathBuf::from("/www/errors"),
                    index: vec![],
                },
                LocationConfig {
                    path: PathBuf::from("/static/images"),
                    ty: Some(LocationConfigType::Prefix),
                    root: PathBuf::from("/www/images"),
                    index: vec!["image.jpg".to_string()],
                },
            ];
        });

        let server = Server::from_config(config)?;

        let res_img = server.router.search("/static/images/logo.png");
        assert!(res_img.is_some());
        assert_eq!(
            res_img.unwrap(),
            RouterHandler {
                root: Arc::from("/www/images"),
                matched_path: Arc::from("/static/images"),
                index: Arc::from(vec![String::from("image.jpg")]),
                match_type: MatchType::Prefix,
            },
            "Should match the longest prefix (/static/images)"
        );

        let res_static = server.router.search("/static/style.css");
        assert!(res_static.is_some());
        assert_eq!(
            res_static.unwrap(),
            RouterHandler {
                root: Arc::from("/www/static"),
                matched_path: Arc::from("/static"),
                index: Arc::from(vec![String::from("static.html")]),
                match_type: MatchType::Prefix,
            }
        );

        let res_root = server.router.search("/unknown/path");
        assert!(res_root.is_some());

        Ok(())
    }
}
