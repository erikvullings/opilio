//! Runtime diagnostics for configured local and remote resources.

use std::{
    collections::BTreeMap,
    fmt,
    io::{self, Write},
    time::Duration,
};

use serde::Serialize;

use crate::{
    config::Config,
    domain::{Device, PowerProvider as ConfiguredPowerProvider},
    history::Redactor,
    power::{
        PowerAvailability, PowerProvider,
        shelly::{ReqwestHttpClient as ShellyHttpClient, ShellyProvider},
        wol::{SystemUdpSender, WolProvider},
    },
    service::{
        EnvironmentSecretResolver, ReqwestHttpClient, ServiceCollector, ServiceObservation,
        ServiceState, SshServiceExecutor,
    },
    ssh::{ExecutionOptions, OpenSsh, RemoteInvocation},
    status::ExitStatus,
    target::TargetError,
    telemetry::{ProviderSnapshot, SshTelemetryExecutor, TelemetryCollector},
};

const SSH_TIMEOUT: Duration = Duration::from_secs(8);
const POWER_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorRequest {
    pub target: String,
}

impl Default for DoctorRequest {
    fn default() -> Self {
        Self {
            target: "all".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Warning,
    Fail,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    LocalSsh,
    Reachability,
    Ssh,
    PowerProvider,
    Telemetry,
    Nvidia,
    Uma,
    Docker,
    Systemd,
    Service,
}

impl fmt::Display for CheckKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = serde_json::to_value(self).map_err(|_| fmt::Error)?;
        formatter.write_str(value.as_str().ok_or(fmt::Error)?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticResult {
    pub kind: CheckKind,
    pub status: CheckStatus,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl DiagnosticResult {
    fn new(kind: CheckKind, status: CheckStatus, summary: impl Into<String>) -> Self {
        Self {
            kind,
            status,
            summary: summary.into(),
            suggestion: None,
        }
    }

    fn suggestion(mut self, value: impl Into<String>) -> Self {
        self.suggestion = Some(value.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeObservation {
    Available(String),
    Unavailable(String),
    Unknown(String),
}

impl RuntimeObservation {
    pub fn available(detail: impl Into<String>) -> Self {
        Self::Available(detail.into())
    }

    pub fn unavailable(detail: impl Into<String>) -> Self {
        Self::Unavailable(detail.into())
    }

    pub fn unknown(detail: impl Into<String>) -> Self {
        Self::Unknown(detail.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    Available(String),
    Unsupported(String),
    Unavailable(String),
}

impl Capability {
    pub fn available(detail: impl Into<String>) -> Self {
        Self::Available(detail.into())
    }

    pub fn unsupported(detail: impl Into<String>) -> Self {
        Self::Unsupported(detail.into())
    }

    pub fn unavailable(detail: impl Into<String>) -> Self {
        Self::Unavailable(detail.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Present,
    Absent,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCapabilities {
    pub nvidia: Presence,
    pub unified_memory: bool,
    pub docker: Presence,
    pub systemd: Presence,
}

impl Default for RemoteCapabilities {
    fn default() -> Self {
        Self {
            nvidia: Presence::Unknown,
            unified_memory: false,
            docker: Presence::Unknown,
            systemd: Presence::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceProbe {
    pub reachability: RuntimeObservation,
    pub ssh: RuntimeObservation,
    pub power: Option<RuntimeObservation>,
    pub telemetry: Option<Capability>,
    pub capabilities: RemoteCapabilities,
    pub services: Vec<ServiceObservation>,
}

pub trait DoctorProbe: Sync {
    fn local_ssh(&self) -> Result<String, String>;

    fn diagnose_device(&self, device_name: &str, device: &Device, config: &Config) -> DeviceProbe;
}

#[derive(Debug, Default)]
pub struct SystemDoctorProbe;

impl DoctorProbe for SystemDoctorProbe {
    fn local_ssh(&self) -> Result<String, String> {
        OpenSsh::system()
            .map(|_| "ssh (from PATH)".to_owned())
            .map_err(|error| error.to_string())
    }

    fn diagnose_device(&self, _device_name: &str, device: &Device, config: &Config) -> DeviceProbe {
        let ssh = match OpenSsh::system() {
            Ok(ssh) => ssh,
            Err(error) => {
                return DeviceProbe {
                    reachability: RuntimeObservation::unknown(error.to_string()),
                    ssh: RuntimeObservation::unavailable(error.to_string()),
                    power: probe_power(device),
                    telemetry: device
                        .telemetry
                        .as_ref()
                        .map(|_| Capability::unavailable(error.to_string())),
                    capabilities: RemoteCapabilities::default(),
                    services: Vec::new(),
                };
            }
        };
        let connection = ssh.execute(
            &device.ssh,
            &RemoteInvocation::command("true", &device.shell),
            ExecutionOptions {
                timeout: Some(SSH_TIMEOUT),
                ..ExecutionOptions::default()
            },
        );
        let (reachability, ssh_observation, connected) = classify_ssh(connection);
        let capabilities = if connected {
            probe_remote_capabilities(&ssh, device)
        } else {
            RemoteCapabilities::default()
        };
        let telemetry = device.telemetry.clone().map(|provider| {
            if !connected {
                return Capability::unavailable("telemetry requires a working SSH connection");
            }
            let remote = SshTelemetryExecutor::new(ssh.clone());
            let snapshot = TelemetryCollector::new(&remote).collect(
                &device.ssh,
                &device.shell,
                provider,
                &crate::ssh::CancellationToken::new(),
            );
            match (&device.telemetry, snapshot.nvidia.as_ref()) {
                (Some(crate::domain::Telemetry::Nvidia), Some(nvidia)) => {
                    provider_capability(nvidia)
                }
                _ => provider_capability(&snapshot.system),
            }
        });
        let services = probe_services(&ssh, device, config);
        DeviceProbe {
            reachability,
            ssh: ssh_observation,
            power: probe_power(device),
            telemetry,
            capabilities,
            services,
        }
    }
}

fn provider_capability<T>(snapshot: &ProviderSnapshot<T>) -> Capability {
    match snapshot {
        ProviderSnapshot::Available { .. } => {
            Capability::available("configured telemetry provider is available")
        }
        ProviderSnapshot::Unsupported => {
            Capability::unsupported("configured telemetry provider is unsupported")
        }
        ProviderSnapshot::Unavailable { error } => Capability::unavailable(error),
    }
}

fn classify_ssh(
    result: Result<crate::ssh::ProcessOutput, crate::ssh::SshError>,
) -> (RuntimeObservation, RuntimeObservation, bool) {
    match result {
        Ok(output) if output.success() => (
            RuntimeObservation::available("configured SSH endpoint is reachable"),
            RuntimeObservation::available("SSH authentication and remote command succeeded"),
            true,
        ),
        Ok(output) => {
            let detail = if output.timed_out {
                format!("SSH connection timed out after {SSH_TIMEOUT:?}")
            } else {
                String::from_utf8_lossy(&output.stderr).trim().to_owned()
            };
            let detail = if detail.is_empty() {
                format!("SSH exited with status {:?}", output.exit_code)
            } else {
                detail
            };
            if is_network_failure(&detail) || output.timed_out {
                (
                    RuntimeObservation::unavailable(detail),
                    RuntimeObservation::unknown("SSH was not attempted beyond network connection"),
                    false,
                )
            } else {
                (
                    RuntimeObservation::available("configured SSH endpoint responded"),
                    RuntimeObservation::unavailable(detail),
                    false,
                )
            }
        }
        Err(error) => (
            RuntimeObservation::unknown("reachability could not be determined"),
            RuntimeObservation::unavailable(error.to_string()),
            false,
        ),
    }
}

fn is_network_failure(detail: &str) -> bool {
    let detail = detail.to_ascii_lowercase();
    [
        "connection timed out",
        "operation timed out",
        "no route to host",
        "network is unreachable",
        "could not resolve hostname",
        "connection refused",
    ]
    .iter()
    .any(|needle| detail.contains(needle))
}

fn probe_remote_capabilities(ssh: &OpenSsh, device: &Device) -> RemoteCapabilities {
    const COMMAND: &str = "\
command -v nvidia-smi >/dev/null 2>&1 && echo nvidia=yes || echo nvidia=no
command -v docker >/dev/null 2>&1 && echo docker=yes || echo docker=no
command -v systemctl >/dev/null 2>&1 && echo systemd=yes || echo systemd=no
{ cat /sys/class/dmi/id/product_name 2>/dev/null; cat /proc/device-tree/model 2>/dev/null; nvidia-smi --query-gpu=name --format=csv,noheader 2>/dev/null; }";
    let output = ssh.execute(
        &device.ssh,
        &RemoteInvocation::command(COMMAND, &device.shell),
        ExecutionOptions {
            timeout: Some(SSH_TIMEOUT),
            ..ExecutionOptions::default()
        },
    );
    let Ok(output) = output else {
        return RemoteCapabilities::default();
    };
    if !output.success() {
        return RemoteCapabilities::default();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let presence = |name: &str| {
        if text.lines().any(|line| line == format!("{name}=yes")) {
            Presence::Present
        } else if text.lines().any(|line| line == format!("{name}=no")) {
            Presence::Absent
        } else {
            Presence::Unknown
        }
    };
    let normalized = text.to_ascii_lowercase();
    RemoteCapabilities {
        nvidia: presence("nvidia"),
        unified_memory: normalized.contains("dgx spark") || normalized.contains("gb10"),
        docker: presence("docker"),
        systemd: presence("systemd"),
    }
}

fn probe_power(device: &Device) -> Option<RuntimeObservation> {
    match &device.power {
        Some(ConfiguredPowerProvider::Shelly { .. }) => Some(
            match ShellyProvider::<ShellyHttpClient>::from_device(device, POWER_TIMEOUT) {
                Ok(provider) => {
                    let status = provider.status();
                    match status.availability {
                        PowerAvailability::Reachable => {
                            RuntimeObservation::available("configured Shelly provider is reachable")
                        }
                        PowerAvailability::Unreachable => RuntimeObservation::unavailable(
                            status
                                .error
                                .unwrap_or_else(|| "Shelly provider is unreachable".to_owned()),
                        ),
                        PowerAvailability::Unknown => {
                            RuntimeObservation::unknown("Shelly availability is unknown")
                        }
                    }
                }
                Err(error) => RuntimeObservation::unavailable(error.to_string()),
            },
        ),
        Some(ConfiguredPowerProvider::Wol { .. }) => {
            Some(match WolProvider::<SystemUdpSender>::from_device(device) {
                Ok(provider) if provider.capabilities().can_request_power_on => {
                    RuntimeObservation::available(
                        "Wake-on-LAN is configured; outlet observation is unsupported",
                    )
                }
                Ok(_) => {
                    RuntimeObservation::unavailable("Wake-on-LAN provider cannot request power-on")
                }
                Err(error) => RuntimeObservation::unavailable(error.to_string()),
            })
        }
        None => None,
    }
}

fn probe_services(ssh: &OpenSsh, device: &Device, config: &Config) -> Vec<ServiceObservation> {
    if device.services.is_empty() {
        return Vec::new();
    }
    let remote = SshServiceExecutor::new(ssh.clone());
    let http = match ReqwestHttpClient::new() {
        Ok(http) => http,
        Err(_) => return Vec::new(),
    };
    let secrets = EnvironmentSecretResolver;
    let collector = ServiceCollector::new(&remote, &http, &secrets);
    let cancellation = crate::ssh::CancellationToken::new();
    device
        .services
        .iter()
        .map(|name| collector.collect(name, &config.services()[name], device, &cancellation))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceDiagnostic {
    pub device: String,
    pub site: Option<String>,
    pub ssh: String,
    pub checks: Vec<DiagnosticResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SiteDiagnostic {
    pub site: String,
    pub status: CheckStatus,
    pub summary: String,
    pub affected_devices: Vec<String>,
    pub suggestion: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorSummary {
    pub devices_total: usize,
    pub devices_healthy: usize,
    pub devices_failed: usize,
    pub warnings: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    pub schema_version: u8,
    pub target: String,
    pub local: DiagnosticResult,
    pub sites: Vec<SiteDiagnostic>,
    pub summary: DoctorSummary,
    pub devices: Vec<DeviceDiagnostic>,
}

impl DoctorReport {
    pub fn exit_status(&self) -> ExitStatus {
        if self.local.status == CheckStatus::Fail || self.summary.devices_healthy == 0 {
            ExitStatus::Failed
        } else if self.summary.devices_failed > 0 {
            ExitStatus::PartialSuccess
        } else {
            ExitStatus::Success
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DoctorError {
    #[error(transparent)]
    Target(#[from] TargetError),
}

pub fn collect_doctor(
    config: &Config,
    request: DoctorRequest,
    probe: &dyn DoctorProbe,
) -> Result<DoctorReport, DoctorError> {
    let names = crate::target::Target::resolve(&request.target, config)?;
    let redactor = diagnostic_redactor(config);
    let local = match probe.local_ssh() {
        Ok(path) => DiagnosticResult::new(
            CheckKind::LocalSsh,
            CheckStatus::Pass,
            format!("system OpenSSH available at {path}"),
        ),
        Err(error) => DiagnosticResult::new(
            CheckKind::LocalSsh,
            CheckStatus::Fail,
            redactor.redact(&error),
        )
        .suggestion("Install the OpenSSH client and ensure `ssh` is on PATH."),
    };

    let mut devices = Vec::with_capacity(names.len());
    if local.status == CheckStatus::Fail {
        for name in names {
            let device = &config.devices()[&name];
            devices.push(DeviceDiagnostic {
                device: name,
                site: device.site.clone(),
                ssh: device.ssh.clone(),
                checks: vec![DiagnosticResult::new(
                    CheckKind::Ssh,
                    CheckStatus::Skipped,
                    "runtime checks skipped because local OpenSSH is unavailable",
                )],
            });
        }
    } else {
        for name in names {
            let device = &config.devices()[&name];
            let observation = probe.diagnose_device(&name, device, config);
            devices.push(DeviceDiagnostic {
                device: name,
                site: device.site.clone(),
                ssh: device.ssh.clone(),
                checks: checks_for_device(device, observation, &redactor),
            });
        }
    }
    devices.sort_by(|left, right| left.device.cmp(&right.device));

    let sites = aggregate_unreachable_sites(&devices);
    let devices_failed = if local.status == CheckStatus::Fail {
        devices.len()
    } else {
        devices
            .iter()
            .filter(|device| {
                device
                    .checks
                    .iter()
                    .any(|check| check.status == CheckStatus::Fail)
            })
            .count()
    };
    let warnings = devices
        .iter()
        .flat_map(|device| &device.checks)
        .filter(|check| check.status == CheckStatus::Warning)
        .count();
    Ok(DoctorReport {
        schema_version: 1,
        target: request.target,
        local,
        sites,
        summary: DoctorSummary {
            devices_total: devices.len(),
            devices_healthy: devices.len() - devices_failed,
            devices_failed,
            warnings,
        },
        devices,
    })
}

fn checks_for_device(
    device: &Device,
    observation: DeviceProbe,
    redactor: &Redactor,
) -> Vec<DiagnosticResult> {
    let mut checks = Vec::new();
    let reachable = matches!(observation.reachability, RuntimeObservation::Available(_));
    checks.push(runtime_check(
        CheckKind::Reachability,
        observation.reachability,
        redactor,
        Some("Check the configured SSH host, routing, and site/VPN connectivity."),
    ));
    checks.push(runtime_check(
        CheckKind::Ssh,
        observation.ssh,
        redactor,
        Some("Run `ssh <configured-host>` and correct keys, aliases, or host-key policy."),
    ));

    if device.power.is_some() {
        checks.push(match observation.power {
            Some(value) => runtime_check(
                CheckKind::PowerProvider,
                value,
                redactor,
                Some("Check the configured power provider host, credentials, and current network."),
            ),
            None => DiagnosticResult::new(
                CheckKind::PowerProvider,
                CheckStatus::Skipped,
                "power provider was not probed",
            ),
        });
    }
    if device.telemetry.is_some() {
        checks.push(match observation.telemetry {
            Some(value) => capability_check(CheckKind::Telemetry, value, redactor),
            None => DiagnosticResult::new(
                CheckKind::Telemetry,
                CheckStatus::Skipped,
                "configured telemetry was not probed",
            ),
        });
    }

    checks.extend(capability_checks(&observation.capabilities, reachable));
    for service in observation.services {
        let (status, summary, suggestion) = match service.state {
            ServiceState::Ready => (
                CheckStatus::Pass,
                format!("service `{}` is ready", service.name),
                None,
            ),
            ServiceState::Stopped | ServiceState::Loading | ServiceState::Unknown => (
                CheckStatus::Warning,
                format!("service `{}` is {:?}", service.name, service.state).to_lowercase(),
                Some("Review the configured service probes and remote service state.".to_owned()),
            ),
            ServiceState::Error => (
                CheckStatus::Fail,
                format!(
                    "service `{}` failed: {}",
                    service.name,
                    service_error(&service)
                ),
                Some(
                    "Check the configured service URL/command, credentials, and timeout."
                        .to_owned(),
                ),
            ),
        };
        checks.push(DiagnosticResult {
            kind: CheckKind::Service,
            status,
            summary: redactor.redact(&summary),
            suggestion,
        });
    }
    checks
}

fn runtime_check(
    kind: CheckKind,
    observation: RuntimeObservation,
    redactor: &Redactor,
    suggestion: Option<&str>,
) -> DiagnosticResult {
    let (status, summary) = match observation {
        RuntimeObservation::Available(detail) => (CheckStatus::Pass, detail),
        RuntimeObservation::Unavailable(detail) => (CheckStatus::Fail, detail),
        RuntimeObservation::Unknown(detail) => (CheckStatus::Skipped, detail),
    };
    let mut result = DiagnosticResult::new(kind, status, redactor.redact(&summary));
    if status == CheckStatus::Fail
        && let Some(suggestion) = suggestion
    {
        result = result.suggestion(suggestion);
    }
    result
}

fn capability_check(
    kind: CheckKind,
    capability: Capability,
    redactor: &Redactor,
) -> DiagnosticResult {
    match capability {
        Capability::Available(detail) => {
            DiagnosticResult::new(kind, CheckStatus::Pass, redactor.redact(&detail))
        }
        Capability::Unsupported(detail) => {
            DiagnosticResult::new(kind, CheckStatus::Warning, redactor.redact(&detail)).suggestion(
                "Remove the provider from this device or install/enable its remote dependency.",
            )
        }
        Capability::Unavailable(detail) => {
            DiagnosticResult::new(kind, CheckStatus::Fail, redactor.redact(&detail))
        }
    }
}

fn capability_checks(capabilities: &RemoteCapabilities, reachable: bool) -> Vec<DiagnosticResult> {
    if !reachable {
        return [
            CheckKind::Nvidia,
            CheckKind::Uma,
            CheckKind::Docker,
            CheckKind::Systemd,
        ]
        .into_iter()
        .map(|kind| {
            DiagnosticResult::new(
                kind,
                CheckStatus::Skipped,
                "remote capability check skipped while device is unreachable",
            )
        })
        .collect();
    }
    vec![
        presence_check(CheckKind::Nvidia, capabilities.nvidia, "NVIDIA tooling"),
        if capabilities.unified_memory {
            DiagnosticResult::new(
                CheckKind::Uma,
                CheckStatus::Pass,
                "DGX Spark/GB10 unified-memory architecture detected",
            )
        } else {
            DiagnosticResult::new(
                CheckKind::Uma,
                CheckStatus::Warning,
                "unified-memory architecture not detected",
            )
        },
        presence_check(CheckKind::Docker, capabilities.docker, "Docker"),
        presence_check(CheckKind::Systemd, capabilities.systemd, "systemd"),
    ]
}

fn presence_check(kind: CheckKind, presence: Presence, name: &str) -> DiagnosticResult {
    match presence {
        Presence::Present => {
            DiagnosticResult::new(kind, CheckStatus::Pass, format!("{name} is present"))
        }
        Presence::Absent => DiagnosticResult::new(
            kind,
            CheckStatus::Warning,
            format!("{name} is not present (informational only)"),
        ),
        Presence::Unknown => DiagnosticResult::new(
            kind,
            CheckStatus::Skipped,
            format!("{name} presence could not be determined"),
        ),
    }
}

fn service_error(service: &ServiceObservation) -> String {
    [&service.status, &service.health, &service.info]
        .into_iter()
        .flatten()
        .filter_map(|probe| probe.error.as_deref())
        .next()
        .unwrap_or("probe reported an error")
        .to_owned()
}

fn aggregate_unreachable_sites(devices: &[DeviceDiagnostic]) -> Vec<SiteDiagnostic> {
    let mut grouped = BTreeMap::<&str, Vec<&DeviceDiagnostic>>::new();
    for device in devices {
        if let Some(site) = device.site.as_deref() {
            grouped.entry(site).or_default().push(device);
        }
    }
    grouped
        .into_iter()
        .filter(|(_, devices)| {
            devices.len() > 1
                && devices.iter().all(|device| {
                    device.checks.iter().any(|check| {
                        check.kind == CheckKind::Reachability && check.status == CheckStatus::Fail
                    })
                })
        })
        .map(|(site, devices)| SiteDiagnostic {
            site: site.to_owned(),
            status: CheckStatus::Fail,
            summary: format!(
                "all {} selected devices at this site are unreachable; likely network/VPN issue",
                devices.len()
            ),
            affected_devices: devices.iter().map(|device| device.device.clone()).collect(),
            suggestion: "Connect the site's network/VPN and retry; Opilio will not manage VPNs."
                .to_owned(),
        })
        .collect()
}

fn diagnostic_redactor(config: &Config) -> Redactor {
    let mut values = config.resolved_secret_values();
    for device in config.devices().values() {
        if let Some(ConfiguredPowerProvider::Shelly {
            auth: Some(auth), ..
        }) = &device.power
        {
            values.push(auth.password.to_string());
            values.push(auth.password.environment_variable().to_owned());
        }
    }
    for service in config.services().values() {
        for probe in [service.health.as_ref(), service.info.as_ref()]
            .into_iter()
            .flatten()
        {
            for reference in probe.headers.values() {
                values.push(reference.to_string());
                values.push(reference.environment_variable().to_owned());
            }
        }
    }
    Redactor::new(values)
}

pub fn write_json(output: &mut dyn Write, report: &DoctorReport) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *output, report).map_err(io::Error::other)?;
    writeln!(output)
}

pub fn write_human(output: &mut dyn Write, report: &DoctorReport) -> io::Result<()> {
    writeln!(
        output,
        "Local OpenSSH: {} — {}",
        status_name(report.local.status),
        report.local.summary
    )?;
    if let Some(suggestion) = &report.local.suggestion {
        writeln!(output, "  Suggestion: {suggestion}")?;
    }
    for site in &report.sites {
        writeln!(
            output,
            "Site {}: {} — {}",
            site.site,
            status_name(site.status),
            site.summary
        )?;
        writeln!(output, "  Suggestion: {}", site.suggestion)?;
    }
    for device in &report.devices {
        writeln!(output, "Device {} (SSH {}):", device.device, device.ssh)?;
        for check in &device.checks {
            writeln!(
                output,
                "  {}: {} — {}",
                check.kind,
                status_name(check.status),
                check.summary
            )?;
            if let Some(suggestion) = &check.suggestion {
                writeln!(output, "    Suggestion: {suggestion}")?;
            }
        }
    }
    writeln!(
        output,
        "{} healthy, {} failed, {} warning(s)",
        report.summary.devices_healthy, report.summary.devices_failed, report.summary.warnings
    )
}

const fn status_name(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Pass => "pass",
        CheckStatus::Warning => "warning",
        CheckStatus::Fail => "fail",
        CheckStatus::Skipped => "skipped",
    }
}
