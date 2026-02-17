use compio::BufResult;
use compio::buf::{IoBuf, IoBufMut, IoVectoredBufMut};
use compio::io::{AsyncRead, AsyncWrite};
use damas::ServerContext;
use damas::config::*;
use damas::error::ErrorRegistry;
use damas::http::handle_request;
use damas::index::IndexCache;
use damas::response::error_response;
use damas::router::RouterNode;
use damas::util::sanitize_path;
use minijinja::Environment;
use once_cell::sync::Lazy;
use std::fs::File;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[cfg(test)]
static JINJA_ENV: Lazy<Environment<'static>> = Lazy::new(|| {
    let mut env = Environment::new();
    env.add_template("error", include_str!("../template/error.html"))
        .unwrap();
    env.add_template("index", include_str!("../template/index.html"))
        .unwrap();
    env
});

#[test]
fn test_sanitize_path_valid() {
    let base_root = PathBuf::from("/var/www/html");
    let path = "/index.html";
    let sanitized = sanitize_path(path, &base_root);
    assert_eq!(sanitized, Some(PathBuf::from("/var/www/html/index.html")));
}

#[test]
fn test_sanitize_path_directory_traversal() {
    let base_root = PathBuf::from("/var/www/html");
    let path = "/../../../../etc/passwd";
    let sanitized = sanitize_path(path, &base_root);
    assert_eq!(sanitized, None);
}

#[test]
fn test_sanitize_path_encoded() {
    let base_root = PathBuf::from("/var/www/html");
    let path = "/%2E%2E/%2E%2E/etc/passwd";
    let sanitized = sanitize_path(path, &base_root);
    assert_eq!(sanitized, None);
}

struct RwMock {
    read_buf: &'static [u8],
    write_buf: Vec<u8>,
}

impl RwMock {
    fn new(read_data: &'static [u8]) -> Self {
        RwMock {
            read_buf: read_data,
            write_buf: Vec::new(),
        }
    }
}

impl AsyncRead for RwMock {
    async fn read<B: IoBufMut>(&mut self, buf: B) -> BufResult<usize, B> {
        self.read_buf.read(buf).await
    }

    async fn read_vectored<V: IoVectoredBufMut>(&mut self, buf: V) -> BufResult<usize, V> {
        self.read_buf.read_vectored(buf).await
    }
}

impl AsyncWrite for RwMock {
    async fn write<T: IoBuf>(&mut self, buf: T) -> BufResult<usize, T> {
        self.write_buf.write(buf).await
    }

    async fn flush(&mut self) -> std::io::Result<()> {
        self.write_buf.flush().await
    }

    async fn shutdown(&mut self) -> std::io::Result<()> {
        self.write_buf.shutdown().await
    }
}

async fn create_mock_context<F>(modifier: F) -> (Config, RouterNode, ErrorRegistry)
where
    F: FnOnce(&mut Config),
{
    let mut config = Config {
        server: ServerConfig {
            listen: 80,
            server_name: "localhost".to_string(),
            locations: vec![LocationConfig {
                path: Path::new("/").to_path_buf(),
                root: Path::new("/www/var/html").to_path_buf(),
                index: vec!["index.html".to_string(), "index.htm".to_string()],
                ty: Some(LocationConfigType::Prefix),
                ..Default::default()
            }],
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

    let router = RouterNode::from_config(&config).unwrap();
    let error_registry = ErrorRegistry::new(&JINJA_ENV, 100);
    error_registry.init_with_config(&config).await;

    (config, router, error_registry)
}

#[compio::test]
async fn test_handle_connection_invalid_request() {
    let mut stream = RwMock::new(b"GET HTTP/1.1\r\n\r\n"); // missing path\
    let (config, router, error_registry) = create_mock_context(|c| {
        c.server.locations = vec![LocationConfig {
            path: PathBuf::from("/"),
            ty: Some(LocationConfigType::Prefix),
            root: PathBuf::from("/www/root"),
            index: vec![],
            ..Default::default()
        }]
    })
    .await;
    let context = ServerContext::new(
        config,
        router,
        error_registry.clone(),
        IndexCache::new(&JINJA_ENV, 10),
    );
    let result = handle_request(&mut stream, &context).await;
    assert!(result.unwrap().is_err());
    assert!(
        stream
            .write_buf
            .starts_with(b"HTTP/1.1 400 Bad Request\r\n"),
        "Expected 400 Bad Request, got: {:?}",
        String::from_utf8_lossy(&stream.write_buf)
    );
    assert_eq!(stream.write_buf, error_response(&error_registry, 400).await);
}

#[compio::test]
async fn test_handle_connection_unsupported_method() {
    let mut stream = RwMock::new(b"POST /not_found HTTP/1.1\r\n\r\n");
    let (config, router, error_registry) = create_mock_context(|c| {
        c.server.locations = vec![LocationConfig {
            path: PathBuf::from("/"),
            ty: Some(LocationConfigType::Prefix),
            root: PathBuf::from("/www/root"),
            index: vec![],
            ..Default::default()
        }];
    })
    .await;
    let context = ServerContext::new(
        config,
        router,
        error_registry.clone(),
        IndexCache::new(&JINJA_ENV, 10),
    );
    let result = handle_request(&mut stream, &context).await;
    assert!(result.unwrap().is_err());
    assert!(
        stream
            .write_buf
            .starts_with(b"HTTP/1.1 405 Method Not Allowed\r\n"),
        "Expected 405 Method Not Allowed, got: {:?}",
        String::from_utf8_lossy(&stream.write_buf)
    );
    assert_eq!(
        stream.write_buf,
        error_response(&error_registry, 405).await.as_ref()
    );
}

#[compio::test]
async fn test_handle_connection_ok() {
    let mut stream = RwMock::new(b"GET /index.html HTTP/1.1\r\nHost: example.domain\r\n\r\n");
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("index.html");
    File::create(&file_path).unwrap();

    let (config, router, error_registry) = create_mock_context(|c| {
        c.server.locations = vec![LocationConfig {
            path: PathBuf::from("/"),
            ty: Some(LocationConfigType::Prefix),
            root: dir.path().to_path_buf(),
            index: vec!["index.html".to_string(), "index2.html".to_string()],
            ..Default::default()
        }];
    })
    .await;
    let context = ServerContext::new(
        config,
        router,
        error_registry,
        IndexCache::new(&JINJA_ENV, 10),
    );
    let result = handle_request(&mut stream, &context).await;
    assert!(
        result.is_ok(),
        "Expected Ok, got: {:?}",
        result.unwrap_err()
    );
    assert!(
        stream.write_buf.starts_with(b"HTTP/1.1 200 OK\r\n"),
        "Expected 200 OK, got: {:?}",
        String::from_utf8_lossy(&stream.write_buf)
    );
    assert!(
        stream.write_buf.ends_with(b"\r\n\r\n"),
        "Expected empty body, got: {:?}",
        String::from_utf8_lossy(&stream.write_buf)
    );
}

#[compio::test]
async fn test_index() {
    let mut stream = RwMock::new(b"GET / HTTP/1.1\r\nHost: example.domain\r\n\r\n");
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("index.html");
    File::create(&file_path).unwrap();

    let (config, router, error_registry) = create_mock_context(|c| {
        c.server.locations = vec![LocationConfig {
            path: PathBuf::from("/"),
            ty: Some(LocationConfigType::Prefix),
            root: dir.path().to_path_buf(),
            index: vec!["index.html".to_string(), "index2.html".to_string()],
            ..Default::default()
        }];
    })
    .await;
    let context = ServerContext::new(
        config,
        router,
        error_registry,
        IndexCache::new(&JINJA_ENV, 10),
    );
    let result = handle_request(&mut stream, &context).await;

    assert!(
        result.is_ok(),
        "Expected Ok, got: {:?}",
        result.unwrap_err()
    );
    assert!(
        stream.write_buf.starts_with(b"HTTP/1.1 200 OK\r\n"),
        "Expected 200 OK, got: {:?}",
        String::from_utf8_lossy(&stream.write_buf)
    );
    assert!(
        stream.write_buf.ends_with(b"\r\n\r\n"),
        "Expected empty body, got: {:?}",
        String::from_utf8_lossy(&stream.write_buf)
    );
}
#[test]
fn test_sanitize_path_all_cases() {
    let base_root = PathBuf::from("/var/www/html");

    let cases = vec![
        // (입력값, 기대하는 결과값)
        ("/index.html", Some("/var/www/html/index.html")), // Leading slash
        ("index.html", Some("/var/www/html/index.html")),  // No leading slash
        ("//config.kdl", Some("/var/www/html/config.kdl")), // Double slash
        ("images/../logo.png", Some("/var/www/html/logo.png")), // Traversal
        ("../../../etc/passwd", None),                     // Escape attempt
    ];

    for (input, expected) in cases {
        let result = sanitize_path(input, &base_root);
        let expected_path = expected.map(PathBuf::from);
        assert_eq!(result, expected_path, "Failed on input: {}", input);
    }
}
