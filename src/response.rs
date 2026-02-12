use crate::error::ErrorRegistry;
use bytes::{Bytes, BytesMut};
use compio::fs::Metadata;
use http::StatusCode;

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

#[cfg(test)]
mod tests {
    use crate::error::ErrorRegistry;

    use super::*;

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
}
