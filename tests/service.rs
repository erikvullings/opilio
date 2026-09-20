use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::Arc,
    sync::Mutex,
    thread,
    time::Duration,
};

use opilio::{
    config::{Config, SecretRef, SecretValue},
    service::{
        CommandResponse, DEFAULT_SERVICE_POLL_INTERVAL, HttpRequest, HttpResponse,
        HttpServiceClient, RemoteServiceExecutor, ReqwestHttpClient, SecretResolver,
        ServiceCollector, ServiceState,
    },
    ssh::{
        CancellationToken, OpenSsh, ProcessAdapter, ProcessError, ProcessOutput, ProcessRequest,
    },
};

const BASE_CONFIG: &str = r#"
devices:
  alpha:
    ssh: alpha
    services: [app]
services:
  app:
    status:
      command: app-status
      timeout: 2s
      states:
        stopped: [stopped]
        loading: [starting]
        ready: [running]
        error: [failed]
"#;

#[derive(Default)]
struct FakeRemote {
    responses: Mutex<Vec<Result<CommandResponse, String>>>,
    timeouts: Mutex<Vec<Duration>>,
}

impl FakeRemote {
    fn returning(responses: Vec<Result<CommandResponse, String>>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().rev().collect()),
            ..Self::default()
        }
    }
}

impl RemoteServiceExecutor for FakeRemote {
    fn run(
        &self,
        _target: &str,
        _shell: &str,
        _command: &str,
        timeout: Duration,
        _cancellation: &CancellationToken,
    ) -> Result<CommandResponse, String> {
        self.timeouts.lock().unwrap().push(timeout);
        self.responses.lock().unwrap().pop().unwrap()
    }
}

#[derive(Default)]
struct FakeHttp {
    responses: Mutex<Vec<Result<HttpResponse, String>>>,
}

impl FakeHttp {
    fn returning(responses: Vec<Result<HttpResponse, String>>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().rev().collect()),
        }
    }
}

impl HttpServiceClient for FakeHttp {
    fn get(&self, _request: &HttpRequest) -> Result<HttpResponse, String> {
        self.responses.lock().unwrap().pop().unwrap()
    }
}

struct FakeSecrets;

impl SecretResolver for FakeSecrets {
    fn resolve(&self, reference: &SecretRef) -> Result<SecretValue, String> {
        reference
            .resolve_with(|name| (name == "API_TOKEN").then(|| "super-secret".to_owned()))
            .map_err(|error| error.to_string())
    }
}

fn command(stdout: &str) -> Result<CommandResponse, String> {
    Ok(CommandResponse {
        exit_code: Some(0),
        stdout: stdout.to_owned(),
        stderr: String::new(),
        timed_out: false,
        cancelled: false,
    })
}

fn collect(
    yaml: &str,
    remote: &FakeRemote,
    http: &dyn HttpServiceClient,
) -> opilio::service::ServiceObservation {
    let config = Config::from_yaml(yaml).unwrap();
    ServiceCollector::new(remote, http, &FakeSecrets).collect(
        "app",
        &config.services()["app"],
        &config.devices()["alpha"],
        &CancellationToken::new(),
    )
}

#[test]
fn configured_command_values_normalize_all_supported_states() {
    for (output, expected) in [
        ("stopped\n", ServiceState::Stopped),
        ("starting\n", ServiceState::Loading),
        ("running\n", ServiceState::Ready),
        ("failed\n", ServiceState::Error),
        ("surprising\n", ServiceState::Unknown),
    ] {
        let observation = collect(
            BASE_CONFIG,
            &FakeRemote::returning(vec![command(output)]),
            &FakeHttp::default(),
        );
        assert_eq!(observation.state, expected, "output: {output}");
        assert_eq!(observation.status.as_ref().unwrap().state, expected);
    }
}

#[test]
fn health_and_status_evidence_are_combined_without_hardcoded_provider_semantics() {
    let yaml =
        format!("{BASE_CONFIG}    health:\n      url: http://service/health\n      timeout: 3s\n");
    let loading = collect(
        &yaml,
        &FakeRemote::returning(vec![command("starting")]),
        &FakeHttp::returning(vec![Ok(HttpResponse {
            status: 503,
            body: "warming".to_owned(),
        })]),
    );
    assert_eq!(loading.state, ServiceState::Loading);
    assert_eq!(loading.health.unwrap().state, ServiceState::Error);

    let ready = collect(
        &yaml,
        &FakeRemote::returning(vec![command("running")]),
        &FakeHttp::returning(vec![Ok(HttpResponse {
            status: 204,
            body: String::new(),
        })]),
    );
    assert_eq!(ready.state, ServiceState::Ready);
}

#[test]
fn http_only_health_and_info_combinations_do_not_require_a_remote_command() {
    let yaml = r#"
devices:
  alpha:
    ssh: alpha
    services: [app]
services:
  app:
    health:
      url: http://service/health
    info:
      url: http://service/info
      extract:
        version: /version
"#;
    let observation = collect(
        yaml,
        &FakeRemote::default(),
        &FakeHttp::returning(vec![
            Ok(HttpResponse {
                status: 200,
                body: "healthy".to_owned(),
            }),
            Ok(HttpResponse {
                status: 200,
                body: r#"{"version":3}"#.to_owned(),
            }),
        ]),
    );

    assert_eq!(observation.state, ServiceState::Ready);
    assert!(observation.status.is_none());
    assert_eq!(observation.fields["version"], 3);
}

#[test]
fn command_failures_and_timeouts_are_explicit_errors() {
    let failed = collect(
        BASE_CONFIG,
        &FakeRemote::returning(vec![Ok(CommandResponse {
            exit_code: Some(7),
            stdout: String::new(),
            stderr: "service missing".to_owned(),
            timed_out: false,
            cancelled: false,
        })]),
        &FakeHttp::default(),
    );
    assert_eq!(failed.state, ServiceState::Error);
    assert_eq!(
        failed.status.unwrap().error.as_deref(),
        Some("status command exited with code 7: service missing")
    );

    let remote = FakeRemote::returning(vec![Ok(CommandResponse {
        timed_out: true,
        ..CommandResponse::default()
    })]);
    let timed_out = collect(BASE_CONFIG, &remote, &FakeHttp::default());
    assert_eq!(timed_out.state, ServiceState::Error);
    assert_eq!(
        timed_out.status.unwrap().error.as_deref(),
        Some("status command timed out after 2s")
    );
    assert_eq!(
        remote.timeouts.lock().unwrap().as_slice(),
        [Duration::from_secs(2)]
    );
}

#[test]
fn generic_json_pointer_extracts_model_name_and_other_typed_fields() {
    let yaml = format!(
        "{BASE_CONFIG}    info:\n      url: http://service/v1/models\n      extract:\n        model: /data/0/id\n        owners: /data/0/owned_by\n"
    );
    let observation = collect(
        &yaml,
        &FakeRemote::returning(vec![command("running")]),
        &FakeHttp::returning(vec![Ok(HttpResponse {
            status: 200,
            body: r#"{"data":[{"id":"Qwen/Qwen3-32B","owned_by":"team"}]}"#.to_owned(),
        })]),
    );

    assert_eq!(
        observation.fields,
        BTreeMap::from([
            ("model".to_owned(), serde_json::json!("Qwen/Qwen3-32B")),
            ("owners".to_owned(), serde_json::json!("team")),
        ])
    );
    assert_eq!(observation.info.unwrap().state, ServiceState::Ready);
}

#[test]
fn malformed_info_and_missing_json_fields_are_reported_without_losing_health_state() {
    let yaml = format!(
        "{BASE_CONFIG}    health:\n      url: http://service/health\n    info:\n      url: http://service/info\n      extract:\n        model: /data/0/id\n"
    );
    let malformed = collect(
        &yaml,
        &FakeRemote::returning(vec![command("running")]),
        &FakeHttp::returning(vec![
            Ok(HttpResponse {
                status: 200,
                body: String::new(),
            }),
            Ok(HttpResponse {
                status: 200,
                body: "{not json".to_owned(),
            }),
        ]),
    );

    assert_eq!(malformed.state, ServiceState::Ready);
    assert!(
        malformed
            .info
            .unwrap()
            .error
            .unwrap()
            .contains("malformed JSON")
    );
    assert!(malformed.fields.is_empty());
}

#[test]
fn real_http_client_enforces_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        thread::sleep(Duration::from_millis(100));
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    });
    let client = ReqwestHttpClient::new().unwrap();

    let error = client
        .get(&HttpRequest {
            url: format!("http://{address}/health"),
            headers: BTreeMap::new(),
            timeout: Duration::from_millis(20),
        })
        .unwrap_err();

    assert!(error.contains("timed out after 20ms"), "{error}");
    server.join().unwrap();
}

#[test]
fn credential_values_are_sent_but_never_exposed_in_results_or_debug_output() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let length = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..length]);
        assert!(request.contains("authorization: super-secret"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .unwrap();
    });
    let yaml = format!(
        "{BASE_CONFIG}    health:\n      url: http://{address}/health\n      headers:\n        authorization: \"${{env:API_TOKEN}}\"\n"
    );

    let observation = collect(
        &yaml,
        &FakeRemote::returning(vec![command("running")]),
        &ReqwestHttpClient::new().unwrap(),
    );
    let rendered = format!(
        "{observation:?} {}",
        serde_json::to_string(&observation).unwrap()
    );

    assert_eq!(observation.state, ServiceState::Ready);
    assert!(!rendered.contains("super-secret"));
    assert!(!rendered.contains("API_TOKEN"));
    server.join().unwrap();
}

#[test]
fn missing_credentials_and_http_failures_are_explicit_and_redacted() {
    struct MissingSecrets;
    impl SecretResolver for MissingSecrets {
        fn resolve(&self, _reference: &SecretRef) -> Result<SecretValue, String> {
            Err("missing API_TOKEN with super-secret fallback".to_owned())
        }
    }

    let config = Config::from_yaml(&format!(
        "{BASE_CONFIG}    health:\n      url: http://service/health\n      headers:\n        authorization: \"${{env:API_TOKEN}}\"\n"
    ))
    .unwrap();
    let remote = FakeRemote::returning(vec![command("running")]);
    let http = FakeHttp::returning(vec![]);
    let observation = ServiceCollector::new(&remote, &http, &MissingSecrets).collect(
        "app",
        &config.services()["app"],
        &config.devices()["alpha"],
        &CancellationToken::new(),
    );

    let error = observation.health.unwrap().error.unwrap();
    assert_eq!(
        error,
        "health header `authorization` credential is unavailable"
    );
    assert!(!error.contains("API_TOKEN"));
    assert!(!error.contains("super-secret"));

    let failed = collect(
        &format!("{BASE_CONFIG}    health:\n      url: http://service/health\n"),
        &FakeRemote::returning(vec![command("running")]),
        &FakeHttp::returning(vec![Err("connection refused".to_owned())]),
    );
    assert_eq!(failed.state, ServiceState::Error);
    assert_eq!(
        failed.health.unwrap().error.as_deref(),
        Some("connection refused")
    );
}

#[test]
fn default_service_polling_is_slower_than_system_telemetry() {
    assert!(DEFAULT_SERVICE_POLL_INTERVAL >= Duration::from_secs(5));
}

#[derive(Default)]
struct FakeProcess {
    request: Mutex<Option<ProcessRequest>>,
}

impl ProcessAdapter for FakeProcess {
    fn find_executable(&self, name: &str) -> Option<PathBuf> {
        (name == "ssh").then(|| PathBuf::from("/usr/bin/ssh"))
    }

    fn run(&self, request: &ProcessRequest) -> Result<ProcessOutput, ProcessError> {
        *self.request.lock().unwrap() = Some(request.clone());
        Ok(ProcessOutput {
            exit_code: Some(0),
            stdout: b"running\n".to_vec(),
            ..ProcessOutput::default()
        })
    }

    fn interactive(&self, _request: &ProcessRequest) -> Result<i32, ProcessError> {
        unreachable!()
    }
}

#[test]
fn ssh_service_executor_reuses_remote_command_layer_and_timeout() {
    let process = Arc::new(FakeProcess::default());
    let ssh = OpenSsh::discover(process.clone()).unwrap();
    let executor = opilio::service::SshServiceExecutor::new(ssh);

    let response = executor
        .run(
            "ssh-alias",
            "/bin/bash",
            "app-status",
            Duration::from_secs(9),
            &CancellationToken::new(),
        )
        .unwrap();

    assert_eq!(response.stdout, "running\n");
    let request = process.request.lock().unwrap();
    let request = request.as_ref().unwrap();
    assert_eq!(request.timeout(), Some(Duration::from_secs(9)));
    assert_eq!(
        request.arguments(),
        ["ssh-alias", "'/bin/bash' '-lc' 'app-status'"]
    );
}
