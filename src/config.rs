use miette::{IntoDiagnostic, miette};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

#[derive(knus::Decode, Clone, Debug, PartialEq)]
pub struct Config {
    #[knus(child)]
    pub server: ServerConfig,
    #[knus(child)]
    pub performance: PerformanceConfig,
}

#[derive(knus::Decode, Clone, Debug, Default, PartialEq)]
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

#[derive(knus::Decode, Clone, Debug, PartialEq)]
pub struct TLSConfig {
    #[knus(child, unwrap(argument))]
    pub cert: PathBuf,
    #[knus(child, unwrap(argument))]
    pub key: PathBuf,
}
impl TLSConfig {
    pub fn validate(&self) -> miette::Result<()> {
        check_path_safety(&self.cert, "cert")?;
        check_path_safety(&self.key, "key")?;
        Ok(())
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
    #[knus(child, default=CryptoProvider::Ring, unwrap(argument))]
    pub crypto_provider: CryptoProvider,
    #[knus(child, unwrap(argument))]
    pub connection_buffer_size: usize,
    #[knus(child, unwrap(argument))]
    pub file_read_buffer_size: usize,
    #[knus(child, unwrap(argument))]
    pub max_header_count: usize,
}
#[derive(knus::DecodeScalar, Debug, Clone, Default, PartialEq)]
pub enum CryptoProvider {
    #[default]
    Ring,
    AwsLcRs,
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

fn check_path_safety(target: &Path, field_name: &str) -> miette::Result<()> {
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
    use tempfile;

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
    fn test_tls_config_validate() {
        let valid_tls = TLSConfig {
            cert: PathBuf::from("/etc/tls/cert.pem"),
            key: PathBuf::from("/etc/tls/key.pem"),
        };
        assert!(valid_tls.validate().is_ok());

        let invalid_tls = TLSConfig {
            cert: PathBuf::from("../cert.pem"),
            key: PathBuf::from("/etc/tls/key.pem"),
        };
        assert!(invalid_tls.validate().is_err());
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
        assert_eq!(config.performance.crypto_provider, CryptoProvider::Ring);
        assert_eq!(config.performance.connection_buffer_size, 1024);
    }

    #[test]
    fn test_parse_with_tls_and_performance() {
        let config_str = r#"
            server {
                listen 443
                server-name "example.com"
                tls {
                    cert "/path/to/cert"
                    key "/path/to/key"
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
        assert_eq!(tls.cert, PathBuf::from("/path/to/cert"));
        assert_eq!(tls.key, PathBuf::from("/path/to/key"));

        assert_eq!(config.performance.crypto_provider, CryptoProvider::AwsLcRs);
        assert_eq!(config.performance.connection_buffer_size, 8192);
        assert_eq!(config.performance.file_read_buffer_size, 16384);
        assert_eq!(config.performance.max_header_count, 128);
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
