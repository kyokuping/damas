use crate::{error::ErrorRegistry, index::IndexCache};
use bytes::{Bytes, BytesMut};
use compio::fs::Metadata;
use http::StatusCode;
use std::fmt::Write;
use std::path::PathBuf;

fn build_http_response(status: u16, mime: &str, body: Bytes, keep_alive: bool) -> Bytes {
    let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let reason = status_code.canonical_reason().unwrap_or("Unknown Error");

    let mut res = BytesMut::with_capacity(128 + body.len());

    write!(
        &mut res,
        "HTTP/1.1 {} {}\r\n\
            Content-Type: {}\r\n\
            Content-Length: {}\r\n\
            Connection: {}\r\n\r\n",
        status,
        reason,
        mime,
        body.len(),
        if keep_alive { "keep-alive" } else { "close" }
    )
    .ok();

    res.extend_from_slice(&body);
    res.freeze()
}

pub fn response(metadata: &Metadata, mime: &str, status: u16) -> Bytes {
    let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let reason = status_code.canonical_reason().unwrap_or("Unknown Error");

    let mut res = BytesMut::with_capacity(128);

    write!(
        &mut res,
        "HTTP/1.1 {} {}\r\n\
            Content-Type: {}\r\n\
            Content-Length: {}\r\n\
            Connection: keep-alive\r\n\r\n",
        status,
        reason,
        mime,
        metadata.len(),
    )
    .ok();

    res.freeze()
}

pub async fn error_response(registry: &ErrorRegistry, status: u16) -> Bytes {
    let body = registry.resolve(status).await;
    let mime = "text/html; charset=utf-8";
    build_http_response(status, mime, body, false)
}

pub async fn index_page_response(index_cache: &IndexCache, dir_path: &PathBuf) -> Bytes {
    let index = index_cache
        .render_index(dir_path)
        .await
        .unwrap_or_else(|_| {
            Bytes::from("<html><body><h1>Failed to render index page</h1></body></html>")
        });

    let mime = "text/html; charset=utf-8";
    build_http_response(200, mime, index, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorRegistry;
    use minijinja::Environment;
    use once_cell::sync::Lazy;
    use std::fs::File;
    use tempfile::tempdir;

    static JINJA_ENV: Lazy<Environment<'static>> = Lazy::new(|| {
        let mut env = Environment::new();
        env.add_template("error", include_str!("../template/error.html"))
            .unwrap();
        env
    });

    #[compio::test]
    async fn test_build_full_response_404() {
        let registry = ErrorRegistry::new(&JINJA_ENV, 10);

        let mock_body = Bytes::from("<html>404 Not Found</html>");
        registry.get_cache().insert(404, mock_body.clone()).await;

        let response = error_response(&registry, 404).await;
        let res_str = String::from_utf8_lossy(&response);

        assert!(res_str.starts_with("HTTP/1.1 404 Not Found\r\n"));
        assert!(res_str.contains("Content-Type: text/html; charset=utf-8\r\n"));
        assert!(res_str.contains("Content-Length: 26\r\n"));
        assert!(res_str.contains("Connection: close\r\n\r\n"));
        assert!(res_str.ends_with("<html>404 Not Found</html>"));
    }

    #[compio::test]
    async fn test_build_full_response_unknown_code() {
        let registry = ErrorRegistry::new(&JINJA_ENV, 10);

        let response = error_response(&registry, 999).await;
        let res_str = String::from_utf8_lossy(&response);

        assert!(res_str.contains("999 Unknown Error"));
    }

    #[compio::test]
    async fn test_response() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_file.txt");
        let _file = File::create(&file_path).unwrap();
        let metadata = compio::fs::metadata(&file_path).await.unwrap();

        let mime_type = "text/plain";
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

        assert!(res_str.starts_with("HTTP/1.1 200 OK\r\n"));
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

        assert!(res_str.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(res_str.contains("<h1>Failed to render index page</h1>"));
    }
}
