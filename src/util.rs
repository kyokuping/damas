use std::path::Path;
use std::path::{Component, PathBuf};

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
pub fn get_mime_bytes(path: &std::path::Path) -> &str {
    let extension = path
        .as_os_str()
        .as_encoded_bytes()
        .rsplitn(2, |&b| b == b'.')
        .next()
        .unwrap_or(b"");

    match extension {
        b"html" | b"htm" => "text/html; charset=utf-8",
        b"js" | b"mjs" => "text/javascript; charset=utf-8",
        b"css" => "text/css",
        b"json" => "application/json",
        b"png" => "image/png",
        b"jpg" | b"jpeg" => "image/jpeg",
        b"gif" => "image/gif",
        b"svg" => "image/svg+xml",
        b"ico" => "image/x-icon",
        b"txt" => "text/plain; charset=utf-8",
        b"woff2" => "font/woff2",
        _ => "application/octet-stream",
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
            "text/html; charset=utf-8"
        );
        assert_eq!(
            get_mime_bytes(Path::new("script.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(get_mime_bytes(Path::new("styles.css")), "text/css");
        assert_eq!(get_mime_bytes(Path::new("image.png")), "image/png");

        // 확장자가 없는 경우
        assert_eq!(
            get_mime_bytes(Path::new("README")),
            "application/octet-stream"
        );

        // 알 수 없는 확장자
        assert_eq!(
            get_mime_bytes(Path::new("test.xyz")),
            "application/octet-stream"
        );
    }
}
