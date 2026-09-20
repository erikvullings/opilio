//! Generic service health and information probes.

use std::{collections::BTreeMap, env, fmt, time::Duration};

use reqwest::{
    blocking::Client,
    header::{HeaderName, HeaderValue},
};
use serde::Serialize;
use serde_json::Value;

use crate::{
    domain::{Device, HttpProbe, SecretRef, SecretValue, Service, ServiceStateMapping},
    ssh::{CancellationToken, ControlMaster, ExecutionOptions, OpenSsh, RemoteInvocation},
};

pub const DEFAULT_SERVICE_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_SERVICE_POLL_INTERVAL: Duration = Duration::from_secs(7);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Stopped,
    Loading,
    Ready,
    Error,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProbeObservation {
    pub state: ServiceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ServiceObservation {
    pub name: String,
    pub state: ServiceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ProbeObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<ProbeObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<ProbeObservation>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandResponse {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub cancelled: bool,
}

pub trait RemoteServiceExecutor: Sync {
    fn run(
        &self,
        target: &str,
        shell: &str,
        command: &str,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<CommandResponse, String>;
}

#[derive(Debug, Clone)]
pub struct SshServiceExecutor {
    ssh: OpenSsh,
}

impl SshServiceExecutor {
    pub fn new(ssh: OpenSsh) -> Self {
        Self { ssh }
    }

    pub fn system() -> Result<Self, String> {
        OpenSsh::system()
            .map(Self::new)
            .map_err(|error| error.to_string())
    }
}

fn run_ssh_command(
    execute: impl FnOnce(ExecutionOptions) -> Result<crate::ssh::ProcessOutput, crate::ssh::SshError>,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<CommandResponse, String> {
    execute(ExecutionOptions {
        timeout: Some(timeout),
        cancellation: cancellation.clone(),
        ..ExecutionOptions::default()
    })
    .map(|output| CommandResponse {
        exit_code: output.exit_code,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        timed_out: output.timed_out,
        cancelled: output.cancelled,
    })
    .map_err(|error| error.to_string())
}

impl RemoteServiceExecutor for SshServiceExecutor {
    fn run(
        &self,
        target: &str,
        shell: &str,
        command: &str,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<CommandResponse, String> {
        let invocation = RemoteInvocation::command(command, shell);
        run_ssh_command(
            |options| self.ssh.execute(target, &invocation, options),
            timeout,
            cancellation,
        )
    }
}

impl RemoteServiceExecutor for ControlMaster {
    fn run(
        &self,
        _target: &str,
        shell: &str,
        command: &str,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<CommandResponse, String> {
        let invocation = RemoteInvocation::command(command, shell);
        run_ssh_command(
            |options| self.execute(&invocation, options),
            timeout,
            cancellation,
        )
    }
}

#[derive(Clone)]
pub struct HttpRequest {
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub timeout: Duration,
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpRequest")
            .field("url", &self.url)
            .field(
                "headers",
                &self
                    .headers
                    .keys()
                    .map(|name| (name, "[REDACTED]"))
                    .collect::<BTreeMap<_, _>>(),
            )
            .field("timeout", &self.timeout)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

pub trait HttpServiceClient: Sync {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, String>;
}

#[derive(Debug, Clone)]
pub struct ReqwestHttpClient {
    client: Client,
}

impl ReqwestHttpClient {
    pub fn new() -> Result<Self, String> {
        Client::builder()
            .build()
            .map(|client| Self { client })
            .map_err(|error| format!("could not initialize HTTP client: {error}"))
    }
}

impl HttpServiceClient for ReqwestHttpClient {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        let mut builder = self.client.get(&request.url).timeout(request.timeout);
        for (name, value) in &request.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| format!("invalid HTTP header name `{name}`"))?;
            let value = HeaderValue::from_str(value)
                .map_err(|_| format!("invalid value for HTTP header `{name}`"))?;
            builder = builder.header(name, value);
        }
        let response = builder.send().map_err(|error| {
            if error.is_timeout() {
                format!(
                    "HTTP probe timed out after {}",
                    humantime::format_duration(request.timeout)
                )
            } else {
                format!("HTTP probe failed: {error}")
            }
        })?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .map_err(|error| format!("could not read HTTP response: {error}"))?;
        Ok(HttpResponse { status, body })
    }
}

pub trait SecretResolver: Sync {
    fn resolve(&self, reference: &SecretRef) -> Result<SecretValue, String>;
}

#[derive(Debug, Default)]
pub struct EnvironmentSecretResolver;

impl SecretResolver for EnvironmentSecretResolver {
    fn resolve(&self, reference: &SecretRef) -> Result<SecretValue, String> {
        reference
            .resolve_with(|name| env::var(name).ok())
            .map_err(|error| error.to_string())
    }
}

pub struct ServiceCollector<'a> {
    remote: &'a dyn RemoteServiceExecutor,
    http: &'a dyn HttpServiceClient,
    secrets: &'a dyn SecretResolver,
}

impl<'a> ServiceCollector<'a> {
    pub fn new(
        remote: &'a dyn RemoteServiceExecutor,
        http: &'a dyn HttpServiceClient,
        secrets: &'a dyn SecretResolver,
    ) -> Self {
        Self {
            remote,
            http,
            secrets,
        }
    }

    pub fn collect(
        &self,
        name: &str,
        service: &Service,
        device: &Device,
        cancellation: &CancellationToken,
    ) -> ServiceObservation {
        let status = service.status.as_ref().map(|probe| {
            let timeout = probe
                .timeout
                .as_ref()
                .map_or(DEFAULT_SERVICE_TIMEOUT, |duration| duration.0);
            match self.remote.run(
                &device.ssh,
                &device.shell,
                &probe.command,
                timeout,
                cancellation,
            ) {
                Ok(response) => command_observation(response, timeout, &probe.states),
                Err(error) => failed_observation(format!("status command failed: {error}")),
            }
        });
        let health = service
            .health
            .as_ref()
            .map(|probe| self.collect_http("health", probe));
        let (info, fields) = service.info.as_ref().map_or_else(
            || (None, BTreeMap::new()),
            |probe| {
                let (observation, fields) = self.collect_info(probe);
                (Some(observation), fields)
            },
        );
        let state = combined_state(status.as_ref(), health.as_ref());

        ServiceObservation {
            name: name.to_owned(),
            state,
            status,
            health,
            info,
            fields,
        }
    }

    fn collect_http(&self, probe_name: &str, probe: &HttpProbe) -> ProbeObservation {
        match self.http_request(probe_name, probe) {
            Ok(response) => http_observation(&response, &probe.states),
            Err(error) => failed_observation(error),
        }
    }

    fn collect_info(&self, probe: &HttpProbe) -> (ProbeObservation, BTreeMap<String, Value>) {
        let response = match self.http_request("info", probe) {
            Ok(response) => response,
            Err(error) => return (failed_observation(error), BTreeMap::new()),
        };
        let mut observation = http_observation(&response, &probe.states);
        if !(200..300).contains(&response.status) || probe.extract.is_empty() {
            return (observation, BTreeMap::new());
        }
        let json: Value = match serde_json::from_str(&response.body) {
            Ok(json) => json,
            Err(error) => {
                observation.error =
                    Some(format!("info response contained malformed JSON: {error}"));
                return (observation, BTreeMap::new());
            }
        };
        let mut fields = BTreeMap::new();
        let mut missing = Vec::new();
        for (name, pointer) in &probe.extract {
            if let Some(value) = json.pointer(pointer) {
                fields.insert(name.clone(), value.clone());
            } else {
                missing.push(format!("`{name}` at `{pointer}`"));
            }
        }
        if !missing.is_empty() {
            observation.error = Some(format!(
                "info response did not contain configured field(s): {}",
                missing.join(", ")
            ));
        }
        (observation, fields)
    }

    fn http_request(&self, probe_name: &str, probe: &HttpProbe) -> Result<HttpResponse, String> {
        let mut headers = BTreeMap::new();
        for (name, reference) in &probe.headers {
            let value = self
                .secrets
                .resolve(reference)
                .map_err(|_| format!("{probe_name} header `{name}` credential is unavailable"))?;
            headers.insert(name.clone(), value.expose().to_owned());
        }
        self.http.get(&HttpRequest {
            url: probe.url.clone(),
            headers,
            timeout: probe
                .timeout
                .as_ref()
                .map_or(DEFAULT_SERVICE_TIMEOUT, |duration| duration.0),
        })
    }
}

fn command_observation(
    response: CommandResponse,
    timeout: Duration,
    states: &ServiceStateMapping,
) -> ProbeObservation {
    if response.timed_out {
        return failed_observation(format!(
            "status command timed out after {}",
            humantime::format_duration(timeout)
        ));
    }
    if response.cancelled {
        return failed_observation("status command was cancelled".to_owned());
    }
    if response.exit_code != Some(0) {
        let code = response.exit_code.map_or_else(
            || "without an exit code".to_owned(),
            |code| format!("with code {code}"),
        );
        let detail = response.stderr.trim();
        return failed_observation(if detail.is_empty() {
            format!("status command exited {code}")
        } else {
            format!("status command exited {code}: {detail}")
        });
    }
    ProbeObservation {
        state: classify(&response.stdout, states),
        http_status: None,
        error: None,
    }
}

fn http_observation(response: &HttpResponse, states: &ServiceStateMapping) -> ProbeObservation {
    let successful = (200..300).contains(&response.status);
    let configured_state = classify(&response.body, states);
    let state = if configured_state != ServiceState::Unknown {
        configured_state
    } else if successful {
        ServiceState::Ready
    } else {
        ServiceState::Error
    };
    ProbeObservation {
        state,
        http_status: Some(response.status),
        error: (!successful).then(|| format!("HTTP probe returned status {}", response.status)),
    }
}

fn failed_observation(error: String) -> ProbeObservation {
    ProbeObservation {
        state: ServiceState::Error,
        http_status: None,
        error: Some(error),
    }
}

fn classify(evidence: &str, states: &ServiceStateMapping) -> ServiceState {
    let trimmed = evidence.trim();
    let json = serde_json::from_str::<Value>(trimmed).ok();
    let matches = |candidates: &[String]| {
        candidates.iter().any(|candidate| {
            trimmed.eq_ignore_ascii_case(candidate)
                || json
                    .as_ref()
                    .is_some_and(|value| json_contains(value, candidate))
        })
    };
    if matches(&states.error) {
        ServiceState::Error
    } else if matches(&states.stopped) {
        ServiceState::Stopped
    } else if matches(&states.loading) {
        ServiceState::Loading
    } else if matches(&states.ready) {
        ServiceState::Ready
    } else {
        ServiceState::Unknown
    }
}

fn json_contains(value: &Value, candidate: &str) -> bool {
    match value {
        Value::String(value) => value.eq_ignore_ascii_case(candidate),
        Value::Array(values) => values.iter().any(|value| json_contains(value, candidate)),
        Value::Object(values) => values.values().any(|value| json_contains(value, candidate)),
        Value::Bool(value) => candidate.eq_ignore_ascii_case(&value.to_string()),
        Value::Number(value) => candidate == value.to_string(),
        Value::Null => candidate.eq_ignore_ascii_case("null"),
    }
}

fn combined_state(
    status: Option<&ProbeObservation>,
    health: Option<&ProbeObservation>,
) -> ServiceState {
    let status = status.map(|probe| probe.state);
    let health = health.map(|probe| probe.state);
    match (status, health) {
        (Some(ServiceState::Stopped), _) => ServiceState::Stopped,
        (Some(ServiceState::Error), _) => ServiceState::Error,
        (Some(ServiceState::Loading), Some(ServiceState::Ready)) => ServiceState::Ready,
        (Some(ServiceState::Loading), _) => ServiceState::Loading,
        (Some(ServiceState::Ready), Some(ServiceState::Error)) => ServiceState::Error,
        (_, Some(state)) => state,
        (Some(state), None) => state,
        (None, None) => ServiceState::Unknown,
    }
}
