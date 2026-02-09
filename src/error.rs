use crate::config::Config;
use bytes::{Bytes, BytesMut};
use compio::{buf::buf_try, fs::File, io::AsyncReadAtExt};
use http::StatusCode;
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug)]
pub struct ErrorRegistry {
    pub error_pages: HashMap<u16, Bytes>,
}

impl ErrorRegistry {
    pub async fn from_config(config: &Config) -> Result<Self, anyhow::Error> {
        let mut error_pages = HashMap::new();

        for error_page in &config.server.error_pages {
            let root = PathBuf::from(&error_page.root);
            for entry in &error_page.files.codes {
                let path = root.join(&entry.file);
                let contents = async move {
                    let error_file = File::open(path).await?;
                    let contents = Vec::new();
                    let (_, contents) = buf_try!(@try error_file.read_to_end_at(contents, 0).await);
                    Ok::<Bytes, anyhow::Error>(Bytes::from(contents))
                }
                .await;

                error_pages.insert(
                    entry.status,
                    contents.unwrap_or_else(|_| get_internal_default(entry.status)),
                );
            }
        }
        Ok(ErrorRegistry { error_pages })
    }

    pub fn resolve(&self, status: u16) -> Bytes {
        if let Some(contents) = self.error_pages.get(&status) {
            contents.clone()
        } else {
            get_internal_default(status)
        }
    }

    pub fn build_full_response(&self, status: u16) -> Bytes {
        let body = self.resolve(status);

        let mut res = BytesMut::with_capacity(128 + body.len());

        let status_code = StatusCode::from_u16(status)
            .ok()
            .and_then(|code| code.canonical_reason())
            .unwrap_or("Unknown Error");

        res.extend_from_slice(format!("HTTP/1.1 {} {}\r\n", status, status_code).as_bytes());
        res.extend_from_slice(b"Content-Type: text/html; charset=UTF-8\r\n");
        res.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
        res.extend_from_slice(b"Connection: close\r\n\r\n");
        res.extend_from_slice(&body);

        res.freeze()
    }
}

fn get_internal_default(status: u16) -> Bytes {
    let status_code = StatusCode::from_u16(status)
        .ok()
        .and_then(|code| code.canonical_reason())
        .unwrap_or("Unknown Error");
    let error_template = include_str!("default_error.html").to_string();
    let rendered = error_template
        .replace("{status}", &status.to_string())
        .replace("{status_code}", status_code);
    Bytes::from(rendered)
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
    use std::fs::File as StdFile;
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;

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
        let error_registry = ErrorRegistry::from_config(&config).await.unwrap();
        assert!(error_registry.error_pages.contains_key(&400));
        assert!(error_registry.error_pages.contains_key(&401));
        assert!(error_registry.error_pages.contains_key(&402));
        assert!(error_registry.error_pages.contains_key(&403));
        assert!(!error_registry.error_pages.contains_key(&500));
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

        let error_registry = ErrorRegistry::from_config(&config).await.unwrap();
        assert!(error_registry.error_pages.contains_key(&400));
        assert_eq!(
            error_registry.resolve(400),
            Bytes::from("~~400_BAD_REQUEST~~\n")
        );
        assert!(error_registry.error_pages.contains_key(&401));
        assert_eq!(
            error_registry.resolve(401),
            Bytes::from("~~401_UNAUTHORIZED~~\n")
        );
    }

    #[compio::test]
    async fn test_resolve_unregistered_status_code() {
        let config = create_mock_config(|_| {});

        let error_registry = ErrorRegistry::from_config(&config).await.unwrap();
        assert!(!error_registry.error_pages.contains_key(&400));
        assert_eq!(error_registry.resolve(400), get_internal_default(400));
    }

    #[test]
    fn test_build_full_response_404() {
        let mut error_pages = std::collections::HashMap::new();
        error_pages.insert(404, Bytes::from("<html>404 Not Found</html>"));

        let registry = ErrorRegistry { error_pages };

        let response = registry.build_full_response(404);
        let res_str = String::from_utf8_lossy(&response);

        assert!(res_str.starts_with("HTTP/1.1 404 Not Found\r\n"));

        assert!(res_str.contains("Content-Type: text/html; charset=UTF-8\r\n"));
        assert!(res_str.contains("Content-Length: 26\r\n"));
        assert!(res_str.contains("Connection: close\r\n\r\n"));

        assert!(res_str.ends_with("<html>404 Not Found</html>"));
    }

    #[test]
    fn test_build_full_response_unknown_code() {
        let registry = ErrorRegistry {
            error_pages: std::collections::HashMap::new(),
        };

        let response = registry.build_full_response(999);
        let res_str = String::from_utf8_lossy(&response);

        assert!(res_str.contains("999 Unknown Error"));
    }
}
