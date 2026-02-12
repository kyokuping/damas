use crate::config::Config;
use crate::error::ErrorRegistry;
use crate::router::RouterNode;
use anyhow::anyhow;
use compio::buf::{IntoInner, IoBuf, buf_try};
use compio::fs::File;
use compio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use compio::io::{AsyncReadAt, AsyncWriteExt};
use mime_guess::{Mime, from_path};
use std::io::ErrorKind;
use std::path::Path;
use std::path::{Component, PathBuf};

pub mod config;
pub mod error;
pub mod router;
pub mod server;

#[derive(Debug)]
pub struct ServerContext<'a> {
    pub config: &'a Config,
    pub router: &'a RouterNode,
    pub error_registry: &'a ErrorRegistry,
}

pub async fn handle_connection<'a, T: AsyncRead + AsyncWrite>(
    mut stream: T,
    context: ServerContext<'a>,
) -> anyhow::Result<()> {
    let mut buffer = Vec::with_capacity(context.config.server.connection_buffer_size);
    loop {
        let (bytes_read, buf) = buf_try!(@try stream.append(buffer).await);
        buffer = buf;
        if bytes_read == 0 {
            println!("Connection closed by peer");
            return Ok(());
        }

        let mut headers =
            vec![httparse::EMPTY_HEADER; context.config.server.max_header_count].into_boxed_slice();
        let mut request = httparse::Request::new(&mut headers);

        let parse_result = request.parse(&buffer);

        match parse_result {
            Ok(httparse::Status::Complete(_)) => break,
            Ok(httparse::Status::Partial) => continue,
            Err(_) => {
                //let response = b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n";
                let response = context.error_registry.build_full_response(400);
                buf_try!(@try stream.write_all(response).await);
                return Err(anyhow!("Failed to parse request"));
            }
        }
    }
    let mut headers =
        vec![httparse::EMPTY_HEADER; context.config.server.max_header_count].into_boxed_slice();
    let mut request = httparse::Request::new(&mut headers);
    request
        .parse(&buffer)
        .expect("Buffer was previously verified as complete");

    if request.method != Some("GET") {
        //let response = b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n";
        let response = context.error_registry.build_full_response(405);
        buf_try!(@try stream.write_all(response).await);
        return Err(anyhow!(
            "Unsupported HTTP method: {}",
            request.method.unwrap_or("UNKNOWN")
        ));
    }
    if let Some(mut path_str) = request.path {
        let (matched_handler, _remaining_path) = match context.router.search(path_str) {
            Some(res) => res,
            None => {
                let response = context.error_registry.build_full_response(404);
                buf_try!(@try stream.write_all(response).await);
                return Err(anyhow!("No matching route found for path: {}", path_str));
            }
        };
        let base_root = PathBuf::from(&*matched_handler.root);
        if base_root.is_dir() {
            let index_list: Option<&Vec<String>> = context
                .config
                .server
                .locations
                .iter()
                .find(|loc| loc.root == base_root)
                .map(|loc| &loc.index);

            if let Some(index) = index_list {
                for idx in index {
                    if base_root.join(idx).is_file() {
                        path_str = idx;
                        break;
                    }
                }
            }
        }
        let path = sanitize_path(path_str, base_root).ok_or(anyhow!("invalid path: {path_str}"))?;
        let file = match File::open(&path).await {
            Ok(file) => file,
            Err(err) => match err.kind() {
                ErrorKind::NotFound => {
                    //let response = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
                    let response = context.error_registry.build_full_response(404);
                    buf_try!(@try stream.write_all(response).await);
                    return Err(err.into());
                }
                _ => {
                    //let response = b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n";
                    let response = context.error_registry.build_full_response(500);
                    buf_try!(@try stream.write_all(response).await);
                    return Err(err.into());
                }
            },
        };
        let metadata = file.metadata().await?;
        let file_size = metadata.len();

        let headers =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", file_size).into_bytes();

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
    Ok(())
}

pub fn sanitize_path(request_path: &str, base_dir: PathBuf) -> Option<PathBuf> {
    let decoded_path = urlencoding::decode(request_path).ok()?;
    let mut clean_path = PathBuf::new();
    let components = Path::new(decoded_path.as_ref()).components();
    for component in components {
        match component {
            Component::Normal(c) => clean_path.push(c),
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
    Some(final_path)
}

pub fn get_mime_type(path: &str) -> Mime {
    from_path(path).first_or_octet_stream()
}
