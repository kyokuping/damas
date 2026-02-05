use compio::BufResult;
use compio::buf::{IoBuf, IoBufMut, IoVectoredBufMut};
use compio::io::{AsyncRead, AsyncWrite};
use damas::router::RouterNode;
use damas::{
    ServerContext,
    config::{Config, parse_config},
    get_mime_type, handle_connection, sanitize_path,
};
use std::path::PathBuf;

#[test]
fn test_sanitize_path_valid() {
    let base_root = PathBuf::from("/var/www/html");
    let path = "/index.html";
    let sanitized = sanitize_path(path, base_root);
    assert_eq!(sanitized, Some(PathBuf::from("/var/www/html/index.html")));
}

#[test]
fn test_sanitize_path_directory_traversal() {
    let base_root = PathBuf::from("/var/www/html");
    let path = "/../../../../etc/passwd";
    let sanitized = sanitize_path(path, base_root);
    assert_eq!(sanitized, None);
}

#[test]
fn test_sanitize_path_encoded() {
    let base_root = PathBuf::from("/var/www/html");
    let path = "/%2E%2E/%2E%2E/etc/passwd";
    let sanitized = sanitize_path(path, base_root);
    assert_eq!(sanitized, None);
}

#[test]
fn test_get_mime_type_html() {
    let mime = get_mime_type("index.html");
    assert_eq!(mime, "text/html");
}

#[test]
fn test_get_mime_type_css() {
    let mime = get_mime_type("style.css");
    assert_eq!(mime, "text/css");
}

#[test]
fn test_get_mime_type_js() {
    let mime = get_mime_type("script.js");
    assert_eq!(mime, "text/javascript");
}

#[test]
fn test_get_mime_type_png() {
    let mime = get_mime_type("image.png");
    assert_eq!(mime, "image/png");
}

#[test]
fn test_get_mime_type_jpeg() {
    let mime = get_mime_type("image.jpg");
    assert_eq!(mime, "image/jpeg");
}

#[test]
fn test_get_mime_type_unknown() {
    let mime = get_mime_type("file.unknown");
    assert_eq!(mime, "application/octet-stream");
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

#[compio::test]
async fn test_handle_connection_invalid_request() {
    let mut stream = RwMock::new(b"GET HTTP/1.1\r\n\r\n"); // missing path
    let config: &'static Config = Box::leak(Box::new(parse_config("./config.kdl").unwrap()));
    let router = RouterNode::from_config(config).unwrap();
    let context = ServerContext {
        config,
        router: &router,
    };
    let result = handle_connection(&mut stream, context).await;
    assert!(result.is_err());
    assert!(
        stream
            .write_buf
            .starts_with(b"HTTP/1.1 400 Bad Request\r\n"),
        "Expected 400 Bad Request, got: {:?}",
        String::from_utf8_lossy(&stream.write_buf)
    );
}

#[compio::test]
async fn test_handle_connection_unsupported_method() {
    let mut stream = RwMock::new(b"POST /not_found HTTP/1.1\r\n\r\n");
    let config: &'static Config = Box::leak(Box::new(parse_config("./config.kdl").unwrap()));
    let router = RouterNode::from_config(config).unwrap();
    let context = ServerContext {
        config,
        router: &router,
    };
    let result = handle_connection(&mut stream, context).await;
    assert!(result.is_err());
    assert!(
        stream
            .write_buf
            .starts_with(b"HTTP/1.1 405 Method Not Allowed\r\n"),
        "Expected 405 Method Not Allowed, got: {:?}",
        String::from_utf8_lossy(&stream.write_buf)
    );
}

#[compio::test]
async fn test_handle_connection_ok() {
    let mut stream = RwMock::new(b"GET /index.html HTTP/1.1\r\nHost: example.domain\r\n\r\n");
    let config: &'static Config = Box::leak(Box::new(parse_config("./config.kdl").unwrap()));
    let router = RouterNode::from_config(config).unwrap();
    let context = ServerContext {
        config,
        router: &router,
    };
    let result = handle_connection(&mut stream, context).await;
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
