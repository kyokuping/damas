use crate::ServerContext;
use crate::response::{error_response, index_page_response, response};
use crate::util::{get_mime_bytes, sanitize_path};
use anyhow::anyhow;
use compio::buf::{IntoInner, IoBuf, buf_try};
use compio::fs::File;
use compio::io::{AsyncRead, AsyncReadAt, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use std::io::ErrorKind;
use std::path::PathBuf;

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
                let response = error_response(&context.error_registry, 400).await;
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
        let response = error_response(&context.error_registry, 405).await;
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
                let response = error_response(&context.error_registry, 404).await;
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
                if matched_handler.is_auto_index {
                    let response = index_page_response(&context.index_cache, &sanitized_base).await;
                    buf_try!(@try stream.write_all(response).await);
                    return Ok(Err(format!(
                        "Directory listing denied: {:?}",
                        sanitized_base
                    )));
                }
                let response = error_response(&context.error_registry, 403).await;
                buf_try!(@try stream.write_all(response).await);
                return Ok(Err(format!(
                    "Directory listing denied: {:?}",
                    sanitized_base
                )));
            }
        } else if !sanitized_base.is_file() {
            let response = error_response(&context.error_registry, 404).await;
            buf_try!(@try stream.write_all(response).await);
            return Ok(Err(format!("File not found: {:?}", sanitized_base)));
        }

        println!("Path: {:?}", final_file_path);
        let file = match File::open(&final_file_path).await {
            Ok(file) => file,
            Err(err) => match err.kind() {
                ErrorKind::NotFound => {
                    //let response = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
                    let response = error_response(&context.error_registry, 404).await;
                    buf_try!(@try stream.write_all(response).await);
                    return Ok(Err(err.to_string()));
                }
                _ => {
                    //let response = b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n";
                    let response = error_response(&context.error_registry, 500).await;
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
