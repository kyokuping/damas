use crate::{error::ErrorRegistry, index::IndexCache};
use bytes::{Bytes, BytesMut};
use compio::fs::Metadata;
use http::StatusCode;
use std::path::PathBuf;

pub fn response(metadata: &Metadata, mime_type: &[u8], status: u16) -> BytesMut {
    let mut itoa_buf = itoa::Buffer::new();

    let mut header_res = BytesMut::with_capacity(128);
    header_res.extend_from_slice(b"HTTP/1.1 ");
    header_res.extend_from_slice(itoa_buf.format(status).as_bytes());
    header_res.extend_from_slice(b" OK\r\nContent-Type: ");
    header_res.extend_from_slice(mime_type);
    header_res.extend_from_slice(b"\r\nContent-Length: ");
    header_res.extend_from_slice(itoa_buf.format(metadata.len()).as_bytes());
    header_res.extend_from_slice(b"\r\nConnection: keep-alive\r\n\r\n");

    header_res
}

pub fn error_response(registry: &ErrorRegistry, status: u16) -> Bytes {
    let body = registry.resolve(status);
    let mut itoa_buf = itoa::Buffer::new();

    let status_code = StatusCode::from_u16(status)
        .ok()
        .and_then(|code| code.canonical_reason())
        .unwrap_or("Unknown Error");

    let mut res = BytesMut::with_capacity(128 + body.len());

    // HTTP Line: HTTP/1.1 {status} {reason}\r\n
    res.extend_from_slice(b"HTTP/1.1 ");
    res.extend_from_slice(itoa_buf.format(status).as_bytes());
    res.extend_from_slice(b" ");
    res.extend_from_slice(status_code.as_bytes());
    res.extend_from_slice(b"\r\n");

    // Headers
    res.extend_from_slice(b"Content-Type: text/html; charset=utf-8\r\n");
    res.extend_from_slice(b"Content-Length: ");
    res.extend_from_slice(itoa_buf.format(body.len()).as_bytes());
    res.extend_from_slice(b"\r\nConnection: close\r\n\r\n");

    // Body
    res.extend_from_slice(&body);

    res.freeze()
}

pub async fn index_page_response(index_cache: &IndexCache, dir_path: &PathBuf) -> Bytes {
    let mut itoa_buf = itoa::Buffer::new();

    let index = index_cache
        .render_index(dir_path)
        .await
        .unwrap_or_else(|_| {
            Bytes::from("<html><body><h1>Failed to render index page</h1></body></html>")
        });

    let mut res = BytesMut::with_capacity(128);
    res.extend_from_slice(b"HTTP/1.1 ");
    res.extend_from_slice(b"200 ");
    res.extend_from_slice(b" OK\r\nContent-Type: ");
    // Headers
    res.extend_from_slice(b"Content-Type: text/html; charset=utf-8\r\n");
    res.extend_from_slice(b"Content-Length: ");
    res.extend_from_slice(itoa_buf.format(index.len()).as_bytes());
    res.extend_from_slice(b"\r\nConnection: keep-alive\r\n\r\n");
    //Body
    res.extend_from_slice(&index);

    res.freeze()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorRegistry;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_build_full_response_404() {
        let mut error_pages = std::collections::HashMap::new();
        error_pages.insert(404, Bytes::from("<html>404 Not Found</html>"));

        let registry = ErrorRegistry { error_pages };

        let response = error_response(&registry, 404);
        let res_str = String::from_utf8_lossy(&response);

        assert!(res_str.starts_with("HTTP/1.1 404 Not Found\r\n"));

        assert!(res_str.contains("Content-Type: text/html; charset=utf-8\r\n"));
        assert!(res_str.contains("Content-Length: 26\r\n"));
        assert!(res_str.contains("Connection: close\r\n\r\n"));

        assert!(res_str.ends_with("<html>404 Not Found</html>"));
    }

    #[test]
    fn test_build_full_response_unknown_code() {
        let registry = ErrorRegistry {
            error_pages: std::collections::HashMap::new(),
        };

        let response = error_response(&registry, 999);
        let res_str = String::from_utf8_lossy(&response);

        assert!(res_str.contains("999 Unknown Error"));
    }

    #[compio::test]
    async fn test_response() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_file.txt");
        let _file = File::create(&file_path).unwrap();
        let metadata = compio::fs::metadata(&file_path).await.unwrap();

        let mime_type = b"text/plain";
        let status = 200;

        let response = response(&metadata, mime_type, status);
        let res_str = String::from_utf8_lossy(&response);

        assert!(res_str.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(res_str.contains("Content-Type: text/plain\r\n"));
        assert!(res_str.contains("Content-Length: 0\r\n"));
        assert!(res_str.contains("Connection: keep-alive\r\n\r\n"));
    }

    #[compio::test]
    async fn test_index_page_response_success() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        File::create(dir.path().join("file1.txt")).unwrap();
        File::create(dir.path().join("file2.txt")).unwrap();

        let index_cache = IndexCache::new(10);
        let response = index_page_response(&index_cache, &dir_path).await;
        let res_str = String::from_utf8_lossy(&response);

        assert!(res_str.starts_with("HTTP/1.1 200  OK\r\n"));
        assert!(res_str.contains("Content-Type: text/html; charset=utf-8\r\n"));
        assert!(res_str.contains("file1.txt"));
        assert!(res_str.contains("file2.txt"));
    }

    #[compio::test]
    async fn test_index_page_response_failure() {
        let dir_path = PathBuf::from("non_existent_directory_for_testing");
        let index_cache = IndexCache::new(10);

        let response = index_page_response(&index_cache, &dir_path).await;
        let res_str = String::from_utf8_lossy(&response);

        assert!(res_str.starts_with("HTTP/1.1 200  OK\r\n"));
        assert!(res_str.contains("<h1>Failed to render index page</h1>"));
    }
}
