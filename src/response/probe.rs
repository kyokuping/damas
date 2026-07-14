use crate::error::DamasError;
use bytes::Bytes;
use compio::buf::buf_try;
use compio::io::{AsyncWrite, AsyncWriteExt};

#[cfg(feature = "observability")]
pub async fn handle_body_response<T: AsyncWrite>(stream: &mut T) -> Result<(), DamasError> {
    let response = Bytes::from_static(
        b"HTTP/1.1 200 OK\r\n\
        Content-Type: application/json\r\n\
        Content-Length: 15\r\n\
        Connection: keep-alive\r\n\r\n\
        {\"status\":\"UP\"}",
    );

    buf_try!(@try stream.write_all(response).await);
    Ok(())
}

#[cfg(feature = "observability")]
pub async fn metrics_body_response<T: AsyncWrite>(stream: &mut T) -> Result<(), DamasError> {
    let metrics_data = "damas_http_requests_total 1\n";
    let body_len = metrics_data.len();

    let mut response = bytes::BytesMut::with_capacity(128 + body_len);

    std::fmt::write(
        &mut response,
        format_args!(
            "HTTP/1.1 200 OK\r\n\
            Content-Type: text/plain; version=0.0.4; charset=utf-8\r\n\
            Content-Length: {}\r\n\
            Connection: keep-alive\r\n\r\n",
            body_len
        ),
    )
    .ok();

    response.extend_from_slice(metrics_data.as_bytes());

    buf_try!(@try stream.write_all(response.freeze()).await);
    Ok(())
}

#[cfg(feature = "observability")]
pub fn observability_error_response() -> Bytes {
    Bytes::from_static(
        b"HTTP/1.1 500 OK\r\n\
        Content-Type: text/html; charset=utf-8\r\n\
        Content-Length: 15\r\n\
        Connection: close\r\n\r\n\
        ",
    )
}
