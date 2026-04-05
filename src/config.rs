use anyhow::{Context, anyhow};
use knus::ast::{Literal, TypeName};
use knus::decode::Kind;
use knus::errors::{DecodeError, ExpectedType};
use knus::span::Spanned;
use knus::traits::ErrorSpan;
use miette::{IntoDiagnostic, miette};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

#[derive(knus::Decode, Debug, PartialEq)]
pub struct Config {
    #[knus(child)]
    pub server: ServerConfig,
    #[knus(child, default)]
    pub performance: PerformanceConfig,
}

#[derive(knus::Decode, Debug, Default, PartialEq)]
pub struct ServerConfig {
    #[knus(child, unwrap(argument))]
    pub listen: u16,
    #[knus(child, unwrap(argument))]
    pub server_name: String,
    #[knus(child)]
    pub tls: Option<TLSConfig>,
    #[knus(children(name = "location"))]
    pub locations: Vec<LocationConfig>,
    #[knus(children(name = "error-page"))]
    pub error_pages: Vec<ErrorPage>,
}

#[derive(knus::Decode, Debug, PartialEq)]
pub struct TLSConfig {
    #[knus(child, unwrap(argument))]
    pub cert: TLSCertificate,
    #[knus(child, unwrap(argument))]
    pub key: TLSPrivateKey,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TLSCertificate(pub CertificateDer<'static>);

impl FromStr for TLSCertificate {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(PathBuf::from(s))
    }
}

impl TryFrom<PathBuf> for TLSCertificate {
    type Error = anyhow::Error;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let data =
            std::fs::read(&path).with_context(|| format!("couldn't find cert file: {:?}", path))?;

        let mut reader = &data[..];
        let cert = rustls_pemfile::certs(&mut reader)
            .next()
            .ok_or_else(|| anyhow!("couldn't find adequate cert file: {:?}", path))??;

        Ok(TLSCertificate(cert))
    }
}

impl From<TLSCertificate> for CertificateDer<'static> {
    fn from(value: TLSCertificate) -> Self {
        value.0
    }
}

impl<S: ErrorSpan> knus::DecodeScalar<S> for TLSCertificate {
    fn raw_decode(
        val: &Spanned<Literal, S>,
        _: &mut knus::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        match &**val {
            Literal::String(s) => {
                let path = Path::new(&**s);
                check_path_safety(path, "cert").map_err(|e| DecodeError::Conversion {
                    span: val.span().clone(),
                    source: e.into(),
                })?;
                CertificateDer::from_pem_file(path)
                    .map(TLSCertificate)
                    .map_err(|e| DecodeError::Conversion {
                        span: val.span().clone(),
                        source: Box::new(e),
                    })
            }
            _ => Err(DecodeError::scalar_kind(Kind::String, val)),
        }
    }
    fn type_check(type_name: &Option<Spanned<TypeName, S>>, ctx: &mut knus::decode::Context<S>) {
        if let Some(typ) = type_name {
            ctx.emit_error(DecodeError::TypeName {
                span: typ.span().clone(),
                found: Some((**typ).clone()),
                expected: ExpectedType::no_type(),
                rust_type: "TLSCertificate",
            });
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct TLSPrivateKey(pub PrivateKeyDer<'static>);

impl TryFrom<PathBuf> for TLSPrivateKey {
    type Error = anyhow::Error;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let data =
            std::fs::read(&path).with_context(|| format!("couldn't find key file: {:?}", path))?;

        let mut reader = &data[..];
        let key = rustls_pemfile::private_key(&mut reader)
            .map_err(|e| anyhow!("PEM parsing failed: {}", e))?
            .ok_or_else(|| anyhow!("key file is empty: {:?}", path))?;

        Ok(TLSPrivateKey(key))
    }
}

impl FromStr for TLSPrivateKey {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(PathBuf::from(s))
    }
}

impl From<TLSPrivateKey> for PrivateKeyDer<'static> {
    fn from(value: TLSPrivateKey) -> Self {
        value.0
    }
}

impl Clone for TLSPrivateKey {
    fn clone(&self) -> Self {
        TLSPrivateKey(self.0.clone_key())
    }
}

impl<S: ErrorSpan> knus::DecodeScalar<S> for TLSPrivateKey {
    fn raw_decode(
        val: &Spanned<Literal, S>,
        _: &mut knus::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        match &**val {
            Literal::String(s) => {
                let path = Path::new(&**s);
                check_path_safety(path, "key").map_err(|e| DecodeError::Conversion {
                    span: val.span().clone(),
                    source: e.into(),
                })?;
                PrivateKeyDer::from_pem_file(path)
                    .map(TLSPrivateKey)
                    .map_err(|e| DecodeError::Conversion {
                        span: val.span().clone(),
                        source: e.into(),
                    })
            }
            _ => Err(DecodeError::scalar_kind(Kind::String, val)),
        }
    }

    fn type_check(type_name: &Option<Spanned<TypeName, S>>, ctx: &mut knus::decode::Context<S>) {
        if let Some(typ) = type_name {
            ctx.emit_error(DecodeError::TypeName {
                span: typ.span().clone(),
                found: Some((**typ).clone()),
                expected: ExpectedType::no_type(),
                rust_type: "TLSPrivateKey",
            });
        }
    }
}

#[derive(knus::Decode, Clone, Debug, Default, PartialEq)]
pub struct LocationConfig {
    /// Request URI path
    #[knus(argument)]
    pub path: PathBuf,
    /// Root directory for serving files
    #[knus(child, unwrap(argument))]
    pub root: PathBuf,
    #[knus(child, default = vec![], unwrap(arguments))]
    pub index: Vec<String>,
    #[knus(type_name)]
    pub ty: Option<LocationConfigType>,
    #[knus(child, default = false, unwrap(argument))]
    pub autoindex: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LocationConfigType {
    Exact,
    Prefix,
}

impl FromStr for LocationConfigType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "exact" => Ok(LocationConfigType::Exact),
            "prefix" => Ok(LocationConfigType::Prefix),
            _ => Err(anyhow::anyhow!("Invalid location type: {}", s)),
        }
    }
}
impl LocationConfig {
    pub fn validate(&self) -> miette::Result<()> {
        check_path_safety(&self.path, "path")?;
        check_path_safety(&self.root, "root")?;
        for (i, filename) in self.index.iter().enumerate() {
            if !is_pure_filename(filename) {
                return Err(miette!("config Error: index {},{}", i, filename));
            }
        }
        Ok(())
    }
}

#[derive(knus::Decode, Clone, Debug, PartialEq)]
pub struct ErrorPage {
    #[knus(argument)]
    pub path: PathBuf,
    #[knus(child, unwrap(argument))]
    pub root: PathBuf,
    #[knus(child)]
    pub files: ErrorFiles,
}

#[derive(knus::Decode, Clone, Debug, PartialEq)]
pub struct ErrorFiles {
    #[knus(children(name = "code"))]
    pub codes: Vec<ErrorCodeEntry>,
}

#[derive(knus::Decode, Clone, Debug, PartialEq)]
pub struct ErrorCodeEntry {
    #[knus(argument)]
    pub status: u16,
    #[knus(argument)]
    pub file: PathBuf,
}

#[derive(knus::Decode, Clone, Debug, Default, PartialEq)]
pub struct PerformanceConfig {
    #[knus(child, default, unwrap(argument))]
    pub crypto_provider: CryptoType,
    #[knus(child, default = 4096, unwrap(argument))]
    pub connection_buffer_size: usize,
    #[knus(child, default = 8019, unwrap(argument))]
    pub file_read_buffer_size: usize,
    #[knus(child, default = 64, unwrap(argument))]
    pub max_header_count: usize,
}
#[derive(knus::DecodeScalar, Debug, Clone, PartialEq)]
pub enum CryptoType {
    #[knus(rename = "ring")]
    Ring,
    #[knus(rename = "aws_lc_rs")]
    AwsLcRs,
}

impl Default for CryptoType {
    fn default() -> CryptoType {
        #[cfg(feature = "aws_lc_rs")]
        return CryptoType::AwsLcRs;

        #[cfg(all(not(feature = "aws_lc_rs"), feature = "ring"))]
        return CryptoType::Ring;

        #[cfg(all(not(feature = "aws_lc_rs"), not(feature = "ring")))]
        compile_error!("At least one of 'ring' or 'aws_lc_rs' features must be enabled!");
    }
}

impl From<CryptoType> for rustls::crypto::CryptoProvider {
    fn from(ty: CryptoType) -> Self {
        match ty {
            CryptoType::Ring => {
                #[cfg(feature = "ring")]
                {
                    rustls::crypto::ring::default_provider()
                }
                #[cfg(not(feature = "ring"))]
                {
                    panic!("'ring' feature is not enabled. Check your Cargo.toml");
                }
            }
            CryptoType::AwsLcRs => {
                #[cfg(feature = "aws_lc_rs")]
                {
                    rustls::crypto::aws_lc_rs::default_provider()
                }
                #[cfg(not(feature = "aws_lc_rs"))]
                {
                    panic!("'aws-lc-rs' feature is not enabled. Check your Cargo.toml");
                }
            }
        }
    }
}
impl Config {
    pub fn validate(&self) -> miette::Result<()> {
        for loc in &self.server.locations {
            loc.validate()?;
        }
        Ok(())
    }
}
pub fn parse_config(config_path: &str) -> miette::Result<Config> {
    let kdl_input = std::fs::read_to_string(config_path).into_diagnostic()?;
    let config = knus::parse::<Config>(config_path, &kdl_input)?;
    config.validate()?;
    Ok(config)
}

fn check_path_safety(target: &Path, field_name: &str) -> Result<(), miette::Error> {
    let mut depth = 0;

    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err(miette!(
                        "config Error: ParentDir '{}', {:?}",
                        field_name,
                        target
                    ));
                }
            }
            Component::CurDir => {}
            Component::RootDir => {}
            Component::Prefix(_) => {
                return Err(miette!(
                    "config Error: Prefix '{}', {:?}",
                    field_name,
                    target
                ));
            }
        }
    }
    Ok(())
}
fn is_pure_filename(filename: &str) -> bool {
    let path = Path::new(filename);
    let mut components = path.components();

    match components.next() {
        Some(Component::Normal(_)) => {}
        _ => return false,
    }
    components.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile;

    const INVALID_CERT: &[u8] = b"-----BEGIN CERTIFICATE-----\nfoobar\n-----END CERTIFICATE-----";
    const INVALID_KEY: &[u8] = b"-----BEGIN PRIVATE KEY-----\nfoobar=\n-----END PRIVATE KEY-----";

    fn mock_cert() -> (TLSCertificate, TLSPrivateKey) {
        let mock_cert_path = Path::new("test/fixtures/tls/cert.pem");
        let mock_cert = fs::read_to_string(mock_cert_path).unwrap();
        let mock_key_path = Path::new("test/fixtures/tls/key.pem");
        let mock_key = fs::read_to_string(mock_key_path).unwrap();
        (
            TLSCertificate(CertificateDer::from_pem_slice(mock_cert.as_bytes()).unwrap()),
            TLSPrivateKey(PrivateKeyDer::from_pem_slice(mock_key.as_bytes()).unwrap()),
        )
    }

    #[test]
    fn test_is_pure_filename() {
        assert!(is_pure_filename("file.txt"));
        assert!(is_pure_filename("file"));
        assert!(!is_pure_filename("/path/to/file"));
        assert!(!is_pure_filename("../file"));
        assert!(!is_pure_filename(""));
    }

    #[test]
    fn test_check_path_safety() {
        assert!(check_path_safety(Path::new("/safe/path"), "path").is_ok());
        assert!(check_path_safety(Path::new("/safe/../path"), "path").is_ok());
        assert!(check_path_safety(Path::new("../unsafe/path"), "path").is_err());
        assert!(check_path_safety(Path::new("/unsafe/../../path"), "path").is_err());
    }

    #[test]
    fn test_location_config_validate() {
        let valid_config = LocationConfig {
            path: PathBuf::from("/"),
            root: PathBuf::from("/var/www"),
            index: vec!["index.html".to_string()],
            ty: None,
            ..Default::default()
        };
        assert!(valid_config.validate().is_ok());

        let invalid_index_config = LocationConfig {
            path: PathBuf::from("/"),
            root: PathBuf::from("/var/www"),
            index: vec!["../index.html".to_string()],
            ty: None,
            ..Default::default()
        };
        assert!(invalid_index_config.validate().is_err());
    }

    #[test]
    fn test_parse_config_invalid_path() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.kdl");
        let invalid_config = r#"
                server {
                    listen 80
                    server-name "localhost"
                    location "../unsafe" {
                        root "/usr/share/nginx/html"
                    }
                }
            "#;
        std::fs::write(&config_path, invalid_config).unwrap();
        let result = parse_config(config_path.to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_performance_config_defaults() {
        let config_str = r#"
            server {
                listen 80
                server-name "localhost"
            }
            performance {
                connection-buffer-size 1024
                file-read-buffer-size 2048
                max-header-count 32
            }
        "#;
        let config = knus::parse::<Config>("test.kdl", config_str).unwrap();
        assert_eq!(config.performance.crypto_provider, CryptoType::Ring);
        assert_eq!(config.performance.connection_buffer_size, 1024);
    }

    #[test]
    fn test_parse_with_tls_and_performance() {
        let config_str = r#"
            server {
                listen 443
                server-name "example.com"
                tls {
                    cert "test/fixtures/tls/cert.pem"
                    key "test/fixtures/tls/key.pem"
                }
            }
            performance {
                crypto-provider "aws-lc-rs"
                connection-buffer-size 8192
                file-read-buffer-size 16384
                max-header-count 128
            }
        "#;
        let config = knus::parse::<Config>("test.kdl", config_str).unwrap();

        let tls = config.server.tls.as_ref().unwrap();
        let (cert, key) = mock_cert();
        assert_eq!(tls.cert, cert);
        assert_eq!(tls.key, key);

        assert_eq!(config.performance.crypto_provider, CryptoType::AwsLcRs);
        assert_eq!(config.performance.connection_buffer_size, 8192);
        assert_eq!(config.performance.file_read_buffer_size, 16384);
        assert_eq!(config.performance.max_header_count, 128);
    }

    #[test]
    fn test_parse_with_invalid_tls() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.kdl");
        let invalid_cert = dir.path().join("invalid_cert.pem");
        std::fs::write(&invalid_cert, INVALID_CERT).unwrap();
        let invalid_key = dir.path().join("invalid_key.pem");
        std::fs::write(&invalid_key, INVALID_KEY).unwrap();
        let invalid_config = format!(
            r#"
            server {{
                listen 443
                server-name "example.com"
                tls {{
                    cert "{}"
                    key "{}"
                }}
            }}
            "#,
            invalid_cert.display(),
            invalid_key.display()
        );
        std::fs::write(&config_path, invalid_config).unwrap();
        let result = parse_config(config_path.to_str().unwrap());
        assert!(
            result.is_err(),
            "parsing with invalid tls certificate should fail"
        );
        let err_msg = format!("{:?}", result.err().unwrap());
        assert!(err_msg.contains("base64") || err_msg.contains("decode"));
    }

    #[test]
    fn test_parse() {
        let config = r#"
            server {
                listen 80
                server-name "localhost"
                location "/" {
                    root "/var/www/html"
                    index "index.html" "index.htm"
                    autoindex false
                }
                location "/location" {
                    root "/var/www/html"
                    index "index.html" "index.htm"
                    autoindex false
                }
                error-page "/40x.html" {
                    root "/var/www/html"
                    files {
                        code 400 "400.html"
                        code 401 "unauthorized.html"
                        code 402 "402.html"
                        code 404 "forbidden.html"
                    }
                }
            }
            performance {
                connection-buffer-size 4096
                file-read-buffer-size 8192
                max-header-count 64
            }

        "#;
        let config = match knus::parse::<Config>("config.kdl", config) {
            Ok(config) => config,
            Err(err) => panic!("Failed to parse config: {:?}", miette::Report::new(err)),
        };
        assert_eq!(
            config,
            Config {
                server: ServerConfig {
                    listen: 80,
                    server_name: "localhost".to_string(),
                    tls: None,
                    locations: vec![
                        LocationConfig {
                            path: Path::new("/").to_path_buf(),
                            root: Path::new("/var/www/html").to_path_buf(),
                            index: vec!["index.html".to_string(), "index.htm".to_string()],
                            ty: None,
                            ..Default::default()
                        },
                        LocationConfig {
                            path: Path::new("/location").to_path_buf(),
                            root: Path::new("/var/www/html").to_path_buf(),
                            index: vec!["index.html".to_string(), "index.htm".to_string()],
                            ty: None,
                            ..Default::default()
                        },
                    ],
                    error_pages: vec![ErrorPage {
                        path: Path::new("/40x.html").to_path_buf(),
                        root: Path::new("/var/www/html").to_path_buf(),
                        files: ErrorFiles {
                            codes: vec![
                                ErrorCodeEntry {
                                    status: 400,
                                    file: Path::new("400.html").to_path_buf(),
                                },
                                ErrorCodeEntry {
                                    status: 401,
                                    file: Path::new("unauthorized.html").to_path_buf(),
                                },
                                ErrorCodeEntry {
                                    status: 402,
                                    file: Path::new("402.html").to_path_buf(),
                                },
                                ErrorCodeEntry {
                                    status: 404,
                                    file: Path::new("forbidden.html").to_path_buf(),
                                }
                            ]
                        }
                    },],
                },
                performance: PerformanceConfig {
                    connection_buffer_size: 4096,
                    file_read_buffer_size: 8192,
                    max_header_count: 64,
                    ..Default::default()
                },
            }
        )
    }
}
