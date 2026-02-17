use crate::config::Config;
use bytes::Bytes;
use compio::{buf::buf_try, fs::File, io::AsyncReadAtExt};
use futures::stream::{StreamExt, futures_unordered::FuturesUnordered};
use http::StatusCode;
use minijinja::{Environment, context};
use moka::future::Cache;

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct ErrorRegistry {
    jinja_env: &'static Environment<'static>,
    inner: Cache<u16, Bytes>,
}

impl ErrorRegistry {
    pub fn new(jinja_env: &'static Environment<'static>, max_capacity: u64) -> Self {
        Self {
            jinja_env,
            inner: Cache::builder().max_capacity(max_capacity).build(),
        }
    }

    #[cfg(test)]
    pub fn get_cache(&self) -> &Cache<u16, Bytes> {
        &self.inner
    }

    pub async fn init_with_config(&self, config: &Config) {
        let mut tasks = FuturesUnordered::new();

        for error_page in &config.server.error_pages {
            let root = PathBuf::from(&error_page.root);
            for entry in &error_page.files.codes {
                let path = root.join(&entry.file);
                let status = entry.status;

                tasks.push(async move {
                    let result = async {
                        let error_file = File::open(&path).await?;
                        let vec = Vec::with_capacity(4096);
                        let (_, contents) = buf_try!(@try error_file.read_to_end_at(vec, 0).await);
                        Ok::<Bytes, anyhow::Error>(Bytes::from(contents))
                    }
                    .await;

                    let body = result.unwrap_or_else(|_| self.render_default_template(status));

                    (status, body)
                });
            }
        }

        while let Some((status, contents)) = tasks.next().await {
            self.inner.insert(status, contents).await;
        }
    }

    pub async fn resolve(&self, status: u16) -> Bytes {
        self.inner
            .get_with(status, async move { self.render_default_template(status) })
            .await
    }

    fn render_default_template(&self, status: u16) -> Bytes {
        let status_code = StatusCode::from_u16(status)
            .ok()
            .and_then(|code| code.canonical_reason())
            .unwrap_or("Unknown Error");
        let rendered = self
            .jinja_env
            .get_template("error")
            .and_then(|t| {
                t.render(context! {
                    status_code => status_code,
                    status => status,
                })
            })
            .unwrap_or_else(|_| format!("Error {}", status));

        Bytes::from(rendered)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config::{
            Config, ErrorCodeEntry, ErrorFiles, ErrorPage, LocationConfig, LocationConfigType,
            ServerConfig,
        },
        error::*,
    };
    use compio::BufResult;
    use compio::io::AsyncWriteAtExt;
    use once_cell::sync::Lazy;
    use std::fs::File as StdFile;
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;

    static JINJA_ENV: Lazy<Environment<'static>> = Lazy::new(|| {
        let mut env = Environment::new();
        env.add_template("error", include_str!("../template/error.html"))
            .unwrap();
        env
    });

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
                error_pages: vec![],
                connection_buffer_size: 4096,
                file_read_buffer_size: 8192,
                max_header_count: 64,
            },
        };

        modifier(&mut config);

        config
    }

    #[compio::test]
    async fn test_from_config_correctly_maps_status_to_path() {
        let dir = tempdir().unwrap();
        let file_400_path = dir.path().join("400.html");
        let mut file_400 = File::create(file_400_path).await.unwrap();
        let BufResult(result, _) = file_400.write_all_at("~~400_BAD_REQUEST~~\n", 0).await;
        result.unwrap();
        let file_401_path = dir.path().join("unauthorized.html");
        let mut file_401 = File::create(file_401_path).await.unwrap();
        let BufResult(result, _) = file_401.write_all_at("~~401_UNAUTHORIZED~~\n", 0).await;
        result.unwrap();
        let file_402_path = dir.path().join("402.html");
        let mut file_402 = File::create(file_402_path).await.unwrap();
        let BufResult(result, _) = file_402.write_all_at("~~402_PAYMENT_REQUIRED~~\n", 0).await;
        result.unwrap();
        let file_403_path = dir.path().join("forbidden.html");
        let mut file_403 = File::create(file_403_path).await.unwrap();
        let BufResult(result, _) = file_403.write_all_at("~~403_FORBIDDEN~~\n", 0).await;
        result.unwrap();
        let config = create_mock_config(|c| {
            c.server.error_pages = vec![ErrorPage {
                path: Path::new("/40x.html").to_path_buf(),
                root: dir.path().to_path_buf(),
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
                            status: 403,
                            file: Path::new("forbidden.html").to_path_buf(),
                        },
                    ],
                },
            }]
        });
        let error_registry = ErrorRegistry::new(&JINJA_ENV, 100);
        error_registry.init_with_config(&config).await;
        assert!(error_registry.inner.contains_key(&400));
        assert!(error_registry.inner.contains_key(&401));
        assert!(error_registry.inner.contains_key(&402));
        assert!(error_registry.inner.contains_key(&403));
        assert!(!error_registry.inner.contains_key(&500));
    }

    #[compio::test]
    async fn test_resolve_custom_file_success() {
        let dir = tempdir().unwrap();

        let file_400_path = dir.path().join("400.html");
        let mut file_400 = StdFile::create(file_400_path).unwrap();
        file_400.write_all(b"~~400_BAD_REQUEST~~\n").unwrap();
        file_400.sync_all().unwrap();
        drop(file_400);

        let file_401_path = dir.path().join("unauthorized.html");
        let mut file_401 = StdFile::create(file_401_path).unwrap();
        file_401.write_all(b"~~401_UNAUTHORIZED~~\n").unwrap();
        file_401.sync_all().unwrap();
        drop(file_401);

        let config = create_mock_config(|c| {
            c.server.error_pages = vec![ErrorPage {
                path: Path::new("/40x.html").to_path_buf(),
                root: dir.path().to_path_buf(),
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
                    ],
                },
            }]
        });
        let error_registry = ErrorRegistry::new(&JINJA_ENV, 100);
        error_registry.init_with_config(&config).await;
        assert!(error_registry.inner.contains_key(&400));
        assert_eq!(
            error_registry.resolve(400).await,
            Bytes::from("~~400_BAD_REQUEST~~\n")
        );
        assert!(error_registry.inner.contains_key(&401));
        assert_eq!(
            error_registry.resolve(401).await,
            Bytes::from("~~401_UNAUTHORIZED~~\n")
        );
    }

    #[compio::test]
    async fn test_resolve_unregistered_status_code() {
        let config = create_mock_config(|_| {});
        let error_registry = ErrorRegistry::new(&JINJA_ENV, 100);
        error_registry.init_with_config(&config).await;
        assert!(!error_registry.inner.contains_key(&400));
        assert_eq!(
            error_registry.resolve(400).await,
            error_registry.render_default_template(400)
        );
    }
}
