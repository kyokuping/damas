use crate::ServerContext;
use crate::error::DamasError;
use compio::buf::buf_try;
use compio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use miette::NamedSource;

pub async fn handle_request<T: AsyncRead + AsyncWrite>(
    stream: &mut T,
    context: &ServerContext,
) -> Result<(), DamasError> {
    let mut buffer = Vec::with_capacity(context.config.server.connection_buffer_size);

    loop {
        let (bytes_read, buf) = buf_try!(@try stream.append(buffer).await);
        buffer = buf;
        if bytes_read == 0 {
            tracing::info!("Connection closed by peer");
            return Ok(());
        }

        let mut headers =
            vec![httparse::EMPTY_HEADER; context.config.server.max_header_count].into_boxed_slice();
        let mut request = httparse::Request::new(&mut headers);

        match request.parse(&buffer) {
            Ok(httparse::Status::Complete(_)) => {
                tracing::info!("Request parsed successfully");
                break;
            }
            Ok(httparse::Status::Partial) => {
                tracing::debug!("Partial request, continuing to read");
                continue;
            }
            Err(_e) => {
                let request_str = String::from_utf8_lossy(&buffer);
                return Err(DamasError::RequestParse {
                    src: NamedSource::new("request", request_str.to_string()),
                    span: (0, request_str.len()).into(),
                });
            }
        }
    }

    let mut headers =
        vec![httparse::EMPTY_HEADER; context.config.server.max_header_count].into_boxed_slice();
    let mut request = httparse::Request::new(&mut headers);
    match request.parse(&buffer) {
        Ok(_status) => {
            let method = request.method.unwrap_or("UNKNOWN");
            let uri = request.path.unwrap_or("/");

            tracing::Span::current()
                .record("method", method)
                .record("path", uri);

            tracing::info!("Received request: {} {}", method, uri);
        }
        Err(err) => {
            return Err(DamasError::from_httparse(err, Some(&buffer)));
        }
    }

    if request.method != Some("GET") {
        return Err(DamasError::MethodNotAllowed(format!(
            "Unsupported HTTP method: {}",
            request.method.unwrap_or("UNKNOWN")
        )));
    }
    if let Some(path_str) = request.path {
        let (matched_handler, remaining_path) = match context.router.search(path_str) {
            Some(res) => {
                tracing::info!("Found matching route for path: {}", path_str);
                res
            }
            None => {
                return Err(DamasError::NotFound(format!(
                    "No matching route found for path: {}",
                    path_str
                )));
            }
        };

        matched_handler
            .handle_request(stream, context, path_str, remaining_path)
            .await?;
    }
    Ok(())
}
