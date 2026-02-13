use crate::config::Config;
use crate::error::ErrorRegistry;
use crate::response::{error_response, response};
use crate::router::RouterNode;
use anyhow::anyhow;
use compio::buf::{IntoInner, IoBuf, buf_try};
use compio::fs::File;
use compio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use compio::io::{AsyncReadAt, AsyncWriteExt};
use std::io::ErrorKind;
use std::path::Path;
use std::path::{Component, PathBuf};
use std::sync::Arc;

pub mod config;
pub mod error;
pub mod response;
pub mod router;
pub mod server;

#[derive(Clone, Debug)]
pub struct ServerContext {
    config: Arc<Config>,
    router: Arc<RouterNode>,
    error_registry: Arc<ErrorRegistry>,
}

impl ServerContext {
    pub fn new(config: Config, router: RouterNode, registry: ErrorRegistry) -> Self {
        Self {
            config: Arc::new(config),
            router: Arc::new(router),
            error_registry: Arc::new(registry),
        }
    }
}

pub async fn handle_connection<T: AsyncRead + AsyncWrite>(
    mut stream: T,
    context: ServerContext,
) -> () {
    match handle_request(&mut stream, &context).await {
        Ok(Ok(())) => (),
        Ok(Err(expected)) => {
            println!("Expected error: {}", expected);
        }
        Err(err) => {
            println!("Error handling request: {}", err);
            let response = error_response(&context.error_registry, 500);
            let _ = stream.write_all(response).await;
        }
    }
}

pub async fn handle_request<T: AsyncRead + AsyncWrite>(
    stream: &mut T,
    context: &ServerContext,
) -> anyhow::Result<Result<(), String>> {
    let mut buffer = Vec::with_capacity(context.config.server.connection_buffer_size);
    loop {
        let (bytes_read, buf) = buf_try!(@try stream.append(buffer).await);
        buffer = buf;
        if bytes_read == 0 {
            println!("Connection closed by peer");
            return Ok(Ok(()));
        }

        let mut headers =
            vec![httparse::EMPTY_HEADER; context.config.server.max_header_count].into_boxed_slice();
        let mut request = httparse::Request::new(&mut headers);

        match request.parse(&buffer) {
            Ok(httparse::Status::Complete(_)) => break,
            Ok(httparse::Status::Partial) => continue,
            Err(_) => {
                //let response = b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n";
                let response = error_response(&context.error_registry, 400);
                buf_try!(@try stream.write_all(response).await);
                return Ok(Err("Failed to parse request".to_owned()));
            }
        }
    }
    let mut headers =
        vec![httparse::EMPTY_HEADER; context.config.server.max_header_count].into_boxed_slice();
    let mut request = httparse::Request::new(&mut headers);
    request.parse(&buffer)?;

    if request.method != Some("GET") {
        //let response = b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n";
        let response = error_response(&context.error_registry, 405);
        buf_try!(@try stream.write_all(response).await);
        return Ok(Err(format!(
            "Unsupported HTTP method: {}",
            request.method.unwrap_or("UNKNOWN")
        )));
    }
    if let Some(path_str) = request.path {
        let (matched_handler, mut remaining_path) = match context.router.search(path_str) {
            Some(res) => res,
            None => {
                let response = error_response(&context.error_registry, 404);
                buf_try!(@try stream.write_all(response).await);
                return Ok(Err(format!(
                    "No matching route found for path: {}",
                    path_str
                )));
            }
        };
        let base_root = PathBuf::from(&*matched_handler.root);
        remaining_path = remaining_path.strip_prefix("/").unwrap_or(remaining_path);
        let sanitized_base =
            sanitize_path(remaining_path, &base_root).ok_or(anyhow!("invalid path: {path_str}"))?;

        let mut final_file_path = sanitized_base.clone();

        if sanitized_base.is_dir() {
            println!("direct directory: {:?}", sanitized_base);

            let mut found = false;
            {
                for idx in matched_handler.index.iter() {
                    let index_path = sanitized_base.join(idx);
                    if index_path.is_file() {
                        final_file_path = index_path;
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                let response = error_response(&context.error_registry, 403);
                buf_try!(@try stream.write_all(response).await);
                return Ok(Err(format!(
                    "Directory listing denied: {:?}",
                    sanitized_base
                )));
            }
        } else if !sanitized_base.is_file() {
            let response = error_response(&context.error_registry, 404);
            buf_try!(@try stream.write_all(response).await);
            return Ok(Err(format!("File not found: {:?}", sanitized_base)));
        }

        println!("Path: {:?}", final_file_path);
        let file = match File::open(&final_file_path).await {
            Ok(file) => file,
            Err(err) => match err.kind() {
                ErrorKind::NotFound => {
                    //let response = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
                    let response = error_response(&context.error_registry, 404);
                    buf_try!(@try stream.write_all(response).await);
                    return Ok(Err(err.to_string()));
                }
                _ => {
                    //let response = b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n";
                    let response = error_response(&context.error_registry, 500);
                    buf_try!(@try stream.write_all(response).await);
                    return Ok(Err(err.to_string()));
                }
            },
        };
        let metadata = file.metadata().await?;
        let file_size = metadata.len();
        let mime_type = get_mime_bytes(&final_file_path);

        let headers = response(&metadata, mime_type, 200);

        let (_, _returned_headers) = buf_try!(@try stream.write_all(headers).await);
        let mut pos = 0;
        let mut file_buffer: Vec<u8> =
            Vec::with_capacity(context.config.server.file_read_buffer_size);

        while pos < file_size {
            let (read_bytes, returned_file_buffer) = buf_try!(
                @try file.read_at(file_buffer, pos).await
            );
            if read_bytes == 0 {
                break;
            }

            let (_, returned_buffer) = buf_try!(
                @try stream
                    .write(returned_file_buffer.slice(..read_bytes))
                    .await
            );
            file_buffer = returned_buffer.into_inner();
            pos += read_bytes as u64;
        }
    }
    Ok(Ok(()))
}

pub fn sanitize_path(request_path: &str, base_dir: &Path) -> Option<PathBuf> {
    let decoded_path = urlencoding::decode(request_path).ok()?;

    let mut clean_path = PathBuf::new();
    for component in Path::new(decoded_path.as_ref()).components() {
        match component {
            Component::Normal(c) => {
                clean_path.push(c);
            }
            Component::ParentDir => {
                if !clean_path.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            Component::RootDir => {}
            Component::Prefix(_) => {
                return None;
            }
        }
    }

    let final_path = base_dir.join(clean_path);

    if final_path.starts_with(base_dir) {
        Some(final_path)
    } else {
        None
    }
}
pub fn get_mime_bytes(path: &std::path::Path) -> &'static [u8] {
    let extension = path
        .as_os_str()
        .as_encoded_bytes()
        .rsplitn(2, |&b| b == b'.')
        .next()
        .unwrap_or(b"");

    match extension {
        b"html" | b"htm" => b"text/html; charset=utf-8",
        b"js" | b"mjs" => b"text/javascript; charset=utf-8",
        b"css" => b"text/css",
        b"json" => b"application/json",
        b"png" => b"image/png",
        b"jpg" | b"jpeg" => b"image/jpeg",
        b"gif" => b"image/gif",
        b"svg" => b"image/svg+xml",
        b"ico" => b"image/x-icon",
        b"txt" => b"text/plain; charset=utf-8",
        b"woff2" => b"font/woff2",
        _ => b"application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_get_mime_bytes() {
        assert_eq!(
            get_mime_bytes(Path::new("index.html")),
            b"text/html; charset=utf-8"
        );
        assert_eq!(
            str::from_utf8(get_mime_bytes(Path::new("script.js"))).unwrap(),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(get_mime_bytes(Path::new("styles.css")), b"text/css");
        assert_eq!(get_mime_bytes(Path::new("image.png")), b"image/png");

        // 확장자가 없는 경우
        assert_eq!(
            get_mime_bytes(Path::new("README")),
            b"application/octet-stream"
        );

        // 알 수 없는 확장자
        assert_eq!(
            str::from_utf8(get_mime_bytes(Path::new("test.xyz"))).unwrap(),
            "application/octet-stream"
        );
    }
}
