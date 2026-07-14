use crate::config::Config;
use crate::error::DamasError;
use crate::request::Request;
use crate::response::build_http_response;
use bytes::Bytes;
use compio::io::AsyncRead;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthStatus {
    Ready,
    Starting,
    Unhealthy,
}

#[derive(Clone, Debug)]
pub struct HealthState {
    status: HealthStatus,
    started_at: Instant,
}

impl HealthState {
    pub fn new(status: HealthStatus) -> Self {
        Self {
            status,
            started_at: Instant::now(),
        }
    }
}

pub fn health_response(state: &HealthState) -> Bytes {
    let (status_code, health_status) = match state.status {
        HealthStatus::Ready => (200, "pass"),
        HealthStatus::Starting => (503, "warn"),
        HealthStatus::Unhealthy => (503, "fail"),
    };
    let body = Bytes::from(format!(
        "{{\"status\":\"{}\",\"version\":\"{}\",\"timestamp\":\"{}\",\"durationMs\":{}}}",
        health_status,
        env!("CARGO_PKG_VERSION"),
        utc_timestamp(),
        state.started_at.elapsed().as_millis(),
    ));

    build_http_response(
        status_code,
        "application/json",
        body,
        true,
        &[("Cache-Control", "no-store")],
    )
}

fn utc_timestamp() -> String {
    humantime::format_rfc3339(SystemTime::now()).to_string()
}

#[derive(Clone, Debug)]
pub struct MonitoringContext {
    config: Arc<Config>,
}

impl MonitoringContext {
    pub fn from_config(config: Arc<Config>) -> Option<Self> {
        config.monitoring.as_ref()?;
        Some(Self { config })
    }

    pub async fn read_request<T: AsyncRead>(
        &self,
        stream: &mut T,
    ) -> Result<Option<Request>, DamasError> {
        Request::from_stream(stream, &self.config.performance).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MonitoringConfig, PerformanceConfig, ServerConfig, SystemRoute};

    fn performance_config() -> PerformanceConfig {
        PerformanceConfig {
            connection_buffer_size: 4096,
            max_header_count: 64,
            ..Default::default()
        }
    }

    #[test]
    fn ready_health_response_matches_issue_contract() {
        let response = health_response(&HealthState::new(HealthStatus::Ready));
        let response = String::from_utf8(response.to_vec()).unwrap();

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("Content-Type: application/json\r\n"));
        assert!(response.contains("Cache-Control: no-store\r\n"));
        assert!(response.contains("\"status\":\"pass\""));
        assert!(response.contains(concat!("\"version\":\"", env!("CARGO_PKG_VERSION"), "\"")));
        assert!(response.contains("\"timestamp\":"));
        assert!(response.contains("\"durationMs\":"));
    }

    #[test]
    fn non_ready_health_response_is_service_unavailable() {
        for status in [HealthStatus::Starting, HealthStatus::Unhealthy] {
            let response = health_response(&HealthState::new(status));
            let response = String::from_utf8(response.to_vec()).unwrap();

            assert!(response.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
        }
    }

    #[test]
    fn utc_timestamp_is_rfc3339_utc() {
        let timestamp = utc_timestamp();

        assert!(timestamp.ends_with('Z'));
        assert!(humantime::parse_rfc3339(&timestamp).is_ok());
    }

    #[test]
    fn monitoring_context_is_absent_without_monitoring_config() {
        let config = Arc::new(Config {
            server: ServerConfig {
                listen: 8080,
                server_name: "127.0.0.1".into(),
                ..Default::default()
            },
            performance: performance_config(),
            monitoring: None,
        });

        assert!(MonitoringContext::from_config(config).is_none());
    }

    #[test]
    fn monitoring_context_keeps_the_entire_config() {
        let config = Arc::new(Config {
            server: ServerConfig {
                listen: 8080,
                server_name: "127.0.0.1".into(),
                ..Default::default()
            },
            performance: performance_config(),
            monitoring: Some(MonitoringConfig {
                listen: 9001,
                health_check: Some(SystemRoute("/_health".into())),
                metrics_path: Some(SystemRoute("/_metrics".into())),
            }),
        });

        let context = MonitoringContext::from_config(config.clone()).unwrap();

        assert!(Arc::ptr_eq(&context.config, &config));
        assert_eq!(context.config.server.server_name, "127.0.0.1");
        assert_eq!(context.config.monitoring.as_ref().unwrap().listen, 9001);
        assert_eq!(
            context
                .config
                .monitoring
                .as_ref()
                .unwrap()
                .health_check
                .as_ref()
                .unwrap()
                .as_str(),
            "/_health"
        );
    }

    #[compio::test]
    async fn monitoring_context_creates_request_with_shared_parser() {
        let config = Arc::new(Config {
            server: ServerConfig {
                listen: 8080,
                server_name: "127.0.0.1".into(),
                ..Default::default()
            },
            performance: performance_config(),
            monitoring: Some(MonitoringConfig {
                listen: 9001,
                health_check: Some(SystemRoute("/_health".into())),
                metrics_path: None,
            }),
        });
        let context = MonitoringContext::from_config(config).unwrap();
        let mut stream = &b"GET /_health HTTP/1.1\r\nHost: localhost\r\n\r\n"[..];

        let request = context.read_request(&mut stream).await.unwrap().unwrap();

        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/_health");
    }
}
