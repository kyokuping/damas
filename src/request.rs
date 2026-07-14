use crate::config::PerformanceConfig;
use crate::error::DamasError;
use compio::buf::buf_try;
use compio::io::{AsyncRead, AsyncReadExt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
}

impl Request {
    pub async fn from_stream<T: AsyncRead>(
        stream: &mut T,
        config: &PerformanceConfig,
    ) -> Result<Option<Self>, DamasError> {
        let mut buffer = Vec::with_capacity(config.connection_buffer_size);

        loop {
            let (bytes_read, buf) = buf_try!(@try stream.append(buffer).await);
            buffer = buf;
            if bytes_read == 0 {
                tracing::info!("Connection closed by peer");
                return Ok(None);
            }

            let mut headers =
                vec![httparse::EMPTY_HEADER; config.max_header_count].into_boxed_slice();
            let mut request = httparse::Request::new(&mut headers);

            match request.parse(&buffer) {
                Ok(httparse::Status::Complete(_)) => {
                    tracing::info!("Request parsed successfully");
                    return Ok(Some(Self {
                        method: request.method.unwrap_or("UNKNOWN").to_owned(),
                        path: request.path.unwrap_or("/").to_owned(),
                    }));
                }
                Ok(httparse::Status::Partial) => {
                    tracing::debug!("Partial request, continuing to read");
                }
                Err(error) => return Err(DamasError::from_httparse(error, Some(&buffer))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn performance_config() -> PerformanceConfig {
        PerformanceConfig {
            connection_buffer_size: 1024,
            max_header_count: 16,
            ..Default::default()
        }
    }

    #[compio::test]
    async fn creates_owned_request_from_stream() {
        let mut stream = &b"GET /index.html HTTP/1.1\r\nHost: localhost\r\n\r\n"[..];

        let request = Request::from_stream(&mut stream, &performance_config())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/index.html");
    }

    #[compio::test]
    async fn returns_none_when_peer_closes_without_request() {
        let mut stream = &b""[..];

        let request = Request::from_stream(&mut stream, &performance_config())
            .await
            .unwrap();

        assert_eq!(request, None);
    }
}
