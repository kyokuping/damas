use http::StatusCode;
use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
pub enum DamasError {
    #[error("Not found: {0}")]
    #[diagnostic(
        code(damas::error::not_found),
        url(docsrs),
        help("The requested resource could not be found.")
    )]
    NotFound(String), // 404

    #[error("Forbidden: {0}")]
    #[diagnostic(
        code(damas::error::forbidden),
        url(docsrs),
        help("You do not have permission to access this resource.")
    )]
    Forbidden(String), // 403

    #[error("Method not allowed: {0}")]
    #[diagnostic(
        code(damas::error::method_not_allowed),
        url(docsrs),
        help("The requested method is not allowed for this resource.")
    )]
    MethodNotAllowed(String), // 405

    #[error("I/O error: {0}")]
    #[diagnostic(code(damas::error::io), url(docsrs), help("An I/O error occurred."))]
    Io(std::io::Error), // 503

    #[error("Configuration error: {0}")]
    #[diagnostic(
        code(damas::error::config),
        url(docsrs),
        help("There is an error in the configuration file.")
    )]
    ConfigError(String), // 400

    #[error("Internal error: {0}")]
    #[diagnostic(
        code(damas::error::internal),
        url(docsrs),
        help("An unexpected internal error occurred.")
    )]
    Internal(Box<dyn std::error::Error + Send + Sync>), // 500

    #[error("Failed to parse request")]
    #[diagnostic(
        code(damas::error::request_parse),
        url(docsrs),
        help("The server could not parse the incoming request.")
    )]
    RequestParse {
        #[source_code]
        src: NamedSource<String>,
        #[label("parsing failed here")]
        span: SourceSpan,
    },
}

impl DamasError {
    pub fn status_code(&self) -> u16 {
        match self {
            Self::ConfigError(_) => 400,
            Self::Forbidden(_) => 403,
            Self::NotFound(_) => 404,
            Self::MethodNotAllowed(_) => 405,
            Self::Internal(_) => 500,
            Self::Io(_) => 503,
            Self::RequestParse { .. } => 400,
        }
    }

    pub fn from_code(code: u16) -> Self {
        match code {
            400 => Self::ConfigError("Bad Request".to_string()),
            403 => Self::Forbidden("Forbidden".to_string()),
            404 => Self::NotFound("Not Found".to_string()),
            405 => Self::MethodNotAllowed("Method Not Allowed".to_string()),
            500 => Self::Internal("Internal Server Error".to_string().into()),
            503 => Self::Io(std::io::Error::other("Service Unavailable")),
            _ => Self::Internal(format!("Unknown Error with status code: {}", code).into()),
        }
    }

    pub fn from_httparse(err: httparse::Error, buffer: Option<&[u8]>) -> Self {
        match buffer {
            Some(b) => {
                let request_str = String::from_utf8_lossy(b).to_string();
                DamasError::RequestParse {
                    src: NamedSource::new("HTTP Request", request_str.clone()),
                    span: (0, request_str.len()).into(),
                }
            }
            None => DamasError::Internal(format!("Parsing failed without buffer: {}", err).into()),
        }
    }

    pub fn to_response(&self) -> (StatusCode, String) {
        match self {
            Self::ConfigError(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            Self::MethodNotAllowed(msg) => (StatusCode::METHOD_NOT_ALLOWED, msg.clone()),
            Self::Internal(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
            Self::Io(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
            Self::RequestParse { .. } => {
                (StatusCode::BAD_REQUEST, "Invalid HTTP Request".to_string())
            }
        }
    }
}

impl From<std::io::Error> for DamasError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => DamasError::NotFound("File not found".to_string()),
            std::io::ErrorKind::PermissionDenied => {
                DamasError::Forbidden("Access denied".to_string())
            }
            _ => DamasError::Io(err),
        }
    }
}
