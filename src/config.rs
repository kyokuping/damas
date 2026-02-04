use std::str::FromStr;

use miette::IntoDiagnostic;

#[derive(knus::Decode, Debug, PartialEq)]
pub struct Config {
    #[knus(child)]
    pub server: ServerConfig,
}

#[derive(knus::Decode, Debug, PartialEq)]
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

#[derive(knus::Decode, Debug, PartialEq)]
pub struct LocationConfig {
    #[knus(argument)]
    pub path: String,
    #[knus(child, unwrap(argument))]
    pub root: String,
    #[knus(child, default = vec![], unwrap(arguments))]
    pub index: Vec<String>,
    #[knus(type_name)]
    pub ty: Option<LocationConfigType>,
}

#[derive(Debug, PartialEq)]
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

#[derive(knus::Decode, Debug, PartialEq)]
pub struct ErrorPage {
    #[knus(argument)]
    pub path: String,
    #[knus(child, unwrap(arguments))]
    pub codes: Vec<u16>,
}

pub fn parse_config(config_path: &str) -> miette::Result<Config> {
    let kdl_input = std::fs::read_to_string(config_path).into_diagnostic()?;
    Ok(knus::parse::<Config>(config_path, &kdl_input)?)
}

#[cfg(test)]
mod tests {
    use super::*;

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
                            path: "/".to_string(),
                            root: "/usr/share/nginx/html".to_string(),
                            index: vec!["index.html".to_string(), "index.htm".to_string()],
                            ty: None,
                        },
                        LocationConfig {
                            path: "/50x.html".to_string(),
                            root: "/usr/share/nginx/html".to_string(),
                            index: vec![],
                            ty: Some(LocationConfigType::Exact),
                        },
                    ],
                    error_page: vec![ErrorPage {
                        codes: vec![500, 502, 503, 504],
                        path: "/50x.html".to_string(),
                    },],
                    connection_buffer_size: 4096,
                    file_read_buffer_size: 8192,
                    max_header_count: 64,
                }
            }
        )
    }
}
