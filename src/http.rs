use crate::ServerContext;
use crate::error::DamasError;
use crate::request::Request;
use compio::io::{AsyncRead, AsyncWrite};

pub async fn handle_request<T: AsyncRead + AsyncWrite>(
    stream: &mut T,
    context: &ServerContext,
) -> Result<(), DamasError> {
    let Some(request) = Request::from_stream(stream, &context.config.performance).await? else {
        return Ok(());
    };

    tracing::Span::current()
        .record("method", request.method.as_str())
        .record("path", request.path.as_str());

    tracing::info!("Received request: {} {}", request.method, request.path);

    if request.method != "GET" {
        return Err(DamasError::MethodNotAllowed(format!(
            "Unsupported HTTP method: {}",
            request.method
        )));
    }

    let path = request.path.as_str();
    let (matched_handler, remaining_path) = match context.router.search(path) {
        Some(res) => {
            tracing::info!("Found matching route for path: {}", path);
            res
        }
        None => {
            return Err(DamasError::NotFound(format!(
                "No matching route found for path: {}",
                path
            )));
        }
    };

    matched_handler
        .handle_request(stream, context, path, remaining_path)
        .await?;

    stream.shutdown().await.map_err(|e| {
        tracing::error!("TLS shutdown error: {:?}", e);
        e
    })?;

    Ok(())
}
