use miette::{IntoDiagnostic, miette};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

#[derive(knus::Decode, Debug, PartialEq)]
pub struct Config {
    #[knus(child)]
    pub server: ServerConfig,
}

#[derive(knus::Decode, Clone, Debug, PartialEq)]
pub struct ServerConfig {
    #[knus(child, unwrap(argument))]
    pub listen: u16,
    #[knus(child, unwrap(argument))]
    pub server_name: String,
    #[knus(children(name = "location"))]
    pub locations: Vec<LocationConfig>,
    #[knus(children(name = "error-page"))]
    pub error_page: Vec<ErrorPage>,
    #[knus(child, unwrap(argument))]
    pub connection_buffer_size: usize,
    #[knus(child, unwrap(argument))]
    pub file_read_buffer_size: usize,
    #[knus(child, unwrap(argument))]
    pub max_header_count: usize,
}

#[derive(knus::Decode, Clone, Debug, PartialEq)]
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
        self.check_path_safety(&self.path, "path")?;
        self.check_path_safety(&self.root, "root")?;
        for (i, filename) in self.index.iter().enumerate() {
            if !self.is_pure_filename(filename) {
                return Err(miette!("config Error: index {},{}", i, filename));
            }
        }
        Ok(())
    }
    fn check_path_safety(&self, target: &Path, field_name: &str) -> miette::Result<()> {
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
    fn is_pure_filename(&self, filename: &str) -> bool {
        let path = Path::new(filename);
        let mut components = path.components();

        match components.next() {
            Some(Component::Normal(_)) => {}
            _ => return false,
        }
        components.next().is_none()
    }
}

#[derive(knus::Decode, Clone, Debug, PartialEq)]
pub struct ErrorPage {
    #[knus(argument)]
    pub path: PathBuf,
    #[knus(child, unwrap(arguments))]
    pub codes: Vec<u16>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile;

    #[test]
    fn test_is_pure_filename() {
        let config = LocationConfig {
            path: PathBuf::new(),
            root: PathBuf::new(),
            index: vec![],
            ty: None,
        };
        assert!(config.is_pure_filename("file.txt"));
        assert!(config.is_pure_filename("file"));
        assert!(!config.is_pure_filename("/path/to/file"));
        assert!(!config.is_pure_filename("../file"));
        assert!(!config.is_pure_filename(""));
    }

    #[test]
    fn test_check_path_safety() {
        let config = LocationConfig {
            path: PathBuf::new(),
            root: PathBuf::new(),
            index: vec![],
            ty: None,
        };

        assert!(
            config
                .check_path_safety(Path::new("/safe/path"), "path")
                .is_ok()
        );
        assert!(
            config
                .check_path_safety(Path::new("/safe/../path"), "path")
                .is_ok()
        );
        assert!(
            config
                .check_path_safety(Path::new("../unsafe/path"), "path")
                .is_err()
        );
        assert!(
            config
                .check_path_safety(Path::new("/unsafe/../../path"), "path")
                .is_err()
        );
    }

    #[test]
    fn test_location_config_validate() {
        let valid_config = LocationConfig {
            path: PathBuf::from("/"),
            root: PathBuf::from("/var/www"),
            index: vec!["index.html".to_string()],
            ty: None,
        };
        assert!(valid_config.validate().is_ok());

        let invalid_index_config = LocationConfig {
            path: PathBuf::from("/"),
            root: PathBuf::from("/var/www"),
            index: vec!["../index.html".to_string()],
            ty: None,
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
    fn test_parse() {
        let config = r#"
            server {
                listen 80
                server-name "localhost"
                location "/" {
                    root "/usr/share/nginx/html"
                    index "index.html" "index.htm"
                }
                error-page "/50x.html" {
                  codes 500 502 503 504
                }
                (exact)location "/50x.html" {
                    root "/usr/share/nginx/html"
                }

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
                    locations: vec![
                        LocationConfig {
                            path: Path::new("/").to_path_buf(),
                            root: Path::new("/usr/share/nginx/html").to_path_buf(),
                            index: vec!["index.html".to_string(), "index.htm".to_string()],
                            ty: None,
                        },
                        LocationConfig {
                            path: Path::new("/50x.html").to_path_buf(),
                            root: Path::new("/usr/share/nginx/html").to_path_buf(),
                            index: vec![],
                            ty: Some(LocationConfigType::Exact),
                        },
                    ],
                    error_page: vec![ErrorPage {
                        codes: vec![500, 502, 503, 504],
                        path: Path::new("/50x.html").to_path_buf(),
                    },],
                    connection_buffer_size: 4096,
                    file_read_buffer_size: 8192,
                    max_header_count: 64,
                }
            }
        )
    }
}
