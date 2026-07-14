use damas::monitoring::{HealthState, HealthStatus, health_response};
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const IO_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
struct MonitoringSpec {
    port: u16,
    health: Option<&'static str>,
    metrics: Option<&'static str>,
}

struct DamasProcess {
    child: Child,
    _dir: TempDir,
}

impl DamasProcess {
    fn spawn(application_port: u16, monitoring: Option<MonitoringSpec>) -> Self {
        let dir = tempfile::tempdir().expect("create test directory");
        let root = dir.path().join("www");
        fs::create_dir(&root).expect("create document root");
        fs::write(root.join("index.html"), "ok").expect("write test index");
        fs::write(
            dir.path().join("config.kdl"),
            render_config(application_port, monitoring, &root),
        )
        .expect("write config.kdl");

        let child = Command::new(env!("CARGO_BIN_EXE_damas"))
            .current_dir(dir.path())
            .env("RUST_LOG", "error")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start damas");

        Self { child, _dir: dir }
    }

    fn wait_for_port(&mut self, port: u16) {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let address = localhost(port);

        loop {
            if let Some(status) = self.child.try_wait().expect("poll damas process") {
                panic!("damas exited before {address} was ready: {status}");
            }

            if TcpStream::connect_timeout(&address, POLL_INTERVAL).is_ok() {
                return;
            }

            assert!(
                Instant::now() < deadline,
                "timed out waiting for damas to listen on {address}"
            );
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn wait_for_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + STARTUP_TIMEOUT;

        loop {
            if let Some(status) = self.child.try_wait().expect("poll damas process") {
                return status;
            }

            assert!(
                Instant::now() < deadline,
                "damas did not exit after a listener bind failure"
            );
            thread::sleep(POLL_INTERVAL);
        }
    }
}

impl Drop for DamasProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct RunningServer {
    process: DamasProcess,
    application_port: u16,
    monitoring_port: u16,
}

impl RunningServer {
    fn start(health: Option<&'static str>, metrics: Option<&'static str>) -> Self {
        let application_port = free_port();
        let monitoring_port = distinct_free_port(application_port);
        let mut process = DamasProcess::spawn(
            application_port,
            Some(MonitoringSpec {
                port: monitoring_port,
                health,
                metrics,
            }),
        );
        process.wait_for_port(application_port);
        process.wait_for_port(monitoring_port);

        Self {
            process,
            application_port,
            monitoring_port,
        }
    }
}

fn render_config(
    application_port: u16,
    monitoring: Option<MonitoringSpec>,
    root: &std::path::Path,
) -> String {
    let root = root
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let monitoring = monitoring.map_or_else(String::new, |spec| {
        let health = spec
            .health
            .map_or_else(String::new, |path| format!("    health-check \"{path}\"\n"));
        let metrics = spec
            .metrics
            .map_or_else(String::new, |path| format!("    metrics-path \"{path}\"\n"));

        format!(
            "monitoring {{\n    listen {}\n{}{}}}\n",
            spec.port, health, metrics
        )
    });

    format!(
        "server {{\n    listen {application_port}\n    server-name \"127.0.0.1\"\n    location \"/\" {{\n        root \"{root}\"\n        index \"index.html\"\n    }}\n}}\n{monitoring}"
    )
}

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("bind ephemeral port")
        .local_addr()
        .expect("read ephemeral port")
        .port()
}

fn distinct_free_port(other: u16) -> u16 {
    loop {
        let port = free_port();
        if port != other {
            return port;
        }
    }
}

fn localhost(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

fn http_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect_timeout(&localhost(port), IO_TIMEOUT)
        .unwrap_or_else(|error| panic!("connect to port {port}: {error}"));
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .expect("set read timeout");
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .expect("set write timeout");
    stream.write_all(request.as_bytes()).expect("write request");
    stream
        .shutdown(Shutdown::Write)
        .expect("finish request body");

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read complete response");
    response
}

fn get(port: u16, path: &str) -> String {
    http_request(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn post(port: u16, path: &str) -> String {
    http_request(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
    )
}

fn assert_status(response: &str, status: &str) {
    assert!(
        response.starts_with(&format!("HTTP/1.1 {status}")),
        "expected HTTP {status}, got:\n{response}"
    );
}

fn response_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("response contains header terminator")
}

fn compact_json(body: &str) -> String {
    body.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn scrape_metrics(port: u16) -> String {
    let response = get(port, "/_metrics");
    assert_status(&response, "200 OK");
    response
}

fn metric_value(metrics: &str, name: &str) -> f64 {
    response_body(metrics)
        .lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next()? == name)
                .then(|| fields.next()?.parse::<f64>().ok())
                .flatten()
        })
        .unwrap_or_else(|| panic!("metric {name} not found in:\n{metrics}"))
}

fn wait_for_metric(port: u16, name: &str, predicate: impl Fn(f64) -> bool) -> f64 {
    let deadline = Instant::now() + IO_TIMEOUT;

    loop {
        let value = metric_value(&scrape_metrics(port), name);
        if predicate(value) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "metric {name} did not reach expected state; last value was {value}"
        );
        thread::sleep(POLL_INTERVAL);
    }
}

#[test]
fn monitoring_block_absent_does_not_create_monitoring_listener() {
    let application_port = free_port();
    let unused_monitoring_port = distinct_free_port(application_port);
    let mut process = DamasProcess::spawn(application_port, None);
    process.wait_for_port(application_port);

    assert!(
        TcpStream::connect_timeout(&localhost(unused_monitoring_port), POLL_INTERVAL).is_err(),
        "a monitoring listener was created without a monitoring block"
    );
}

#[test]
fn monitoring_without_endpoints_listens_and_returns_404_for_every_path() {
    let server = RunningServer::start(None, None);

    for path in ["/", "/_health", "/_metrics", "/_anything"] {
        assert_status(&get(server.monitoring_port, path), "404 Not Found");
    }
}

#[test]
fn ready_health_returns_200_no_store_and_required_fields() {
    let server = RunningServer::start(Some("/_health"), None);
    let response = get(server.monitoring_port, "/_health");
    assert_status(&response, "200 OK");
    assert!(response.contains("Content-Type: application/json\r\n"));
    assert!(response.contains("Cache-Control: no-store\r\n"));

    let body = compact_json(response_body(&response));
    assert!(body.contains("\"status\":\"pass\""));
    assert!(body.contains("\"version\":"));
    assert!(body.contains("\"timestamp\":"));
    assert!(body.contains("\"durationMs\":"));
}

#[test]
fn starting_and_unhealthy_health_return_503() {
    for status in [HealthStatus::Starting, HealthStatus::Unhealthy] {
        let state = HealthState::new(status);
        let response = String::from_utf8(health_response(&state).to_vec())
            .expect("health response is valid UTF-8");
        assert_status(&response, "503 Service Unavailable");
    }
}

#[test]
fn metrics_are_encoded_in_prometheus_text_format() {
    let server = RunningServer::start(None, Some("/_metrics"));
    let response = scrape_metrics(server.monitoring_port);

    assert!(response.contains("Content-Type: text/plain; version=0.0.4; charset=utf-8\r\n"));
    assert!(response_body(&response).contains("# HELP "));
    assert!(response_body(&response).contains("# TYPE "));
    assert!(response_body(&response).contains("damas_http_requests_total"));
}

#[test]
fn application_request_increments_request_counter() {
    let server = RunningServer::start(None, Some("/_metrics"));
    let before = metric_value(
        &scrape_metrics(server.monitoring_port),
        "damas_http_requests_total",
    );

    assert_status(&get(server.application_port, "/index.html"), "200 OK");

    let after = wait_for_metric(
        server.monitoring_port,
        "damas_http_requests_total",
        |value| value > before,
    );
    assert_eq!(after, before + 1.0);
}

#[test]
fn active_connection_gauge_increases_and_decreases() {
    let server = RunningServer::start(None, Some("/_metrics"));
    assert_eq!(
        wait_for_metric(
            server.monitoring_port,
            "damas_active_connections",
            |value| value == 0.0
        ),
        0.0
    );

    let mut application_connection = TcpStream::connect(localhost(server.application_port))
        .expect("open application connection");
    application_connection
        .write_all(b"GET /index.html HTTP/1.1\r\nHost: localhost\r\n")
        .expect("write partial request");

    assert!(
        wait_for_metric(
            server.monitoring_port,
            "damas_active_connections",
            |value| value >= 1.0
        ) >= 1.0
    );

    drop(application_connection);
    assert_eq!(
        wait_for_metric(
            server.monitoring_port,
            "damas_active_connections",
            |value| value == 0.0
        ),
        0.0
    );
}

#[test]
fn monitoring_requests_do_not_increment_application_counter() {
    let server = RunningServer::start(Some("/_health"), Some("/_metrics"));
    let before = metric_value(
        &scrape_metrics(server.monitoring_port),
        "damas_http_requests_total",
    );

    assert_status(&get(server.monitoring_port, "/_health"), "200 OK");
    let after = metric_value(
        &scrape_metrics(server.monitoring_port),
        "damas_http_requests_total",
    );

    assert_eq!(after, before);
}

#[test]
fn post_to_health_or_metrics_returns_405() {
    let server = RunningServer::start(Some("/_health"), Some("/_metrics"));

    for path in ["/_health", "/_metrics"] {
        assert_status(
            &post(server.monitoring_port, path),
            "405 Method Not Allowed",
        );
    }
}

#[test]
fn unregistered_monitoring_path_returns_404() {
    let server = RunningServer::start(Some("/_health"), Some("/_metrics"));
    assert_status(
        &get(server.monitoring_port, "/_not-registered"),
        "404 Not Found",
    );
}

#[test]
fn application_and_monitoring_listeners_run_together() {
    let server = RunningServer::start(Some("/_health"), Some("/_metrics"));

    assert_status(&get(server.application_port, "/index.html"), "200 OK");
    assert_status(&get(server.monitoring_port, "/_health"), "200 OK");
}

#[test]
fn application_bind_failure_fails_the_entire_startup() {
    let application_guard = TcpListener::bind(("127.0.0.1", 0)).expect("reserve application port");
    let application_port = application_guard.local_addr().unwrap().port();
    let monitoring_port = distinct_free_port(application_port);
    let mut process = DamasProcess::spawn(
        application_port,
        Some(MonitoringSpec {
            port: monitoring_port,
            health: Some("/_health"),
            metrics: Some("/_metrics"),
        }),
    );

    assert!(!process.wait_for_exit().success());
}

#[test]
fn monitoring_bind_failure_fails_the_entire_startup() {
    let monitoring_guard = TcpListener::bind(("127.0.0.1", 0)).expect("reserve monitoring port");
    let monitoring_port = monitoring_guard.local_addr().unwrap().port();
    let application_port = distinct_free_port(monitoring_port);
    let mut process = DamasProcess::spawn(
        application_port,
        Some(MonitoringSpec {
            port: monitoring_port,
            health: Some("/_health"),
            metrics: Some("/_metrics"),
        }),
    );

    assert!(!process.wait_for_exit().success());
}
