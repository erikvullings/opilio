use std::collections::BTreeMap;

use opilio::{
    config::Config,
    doctor::{
        Capability, CheckStatus, DeviceProbe, DoctorProbe, DoctorRequest, Presence,
        RemoteCapabilities, RuntimeObservation, collect_doctor, write_human, write_json,
    },
    domain::Device,
    service::{ProbeObservation, ServiceObservation, ServiceState},
    status::ExitStatus,
};

const CONFIG: &str = r#"
sites:
  lab:
    label: Lab VPN
devices:
  alpha:
    site: lab
    ssh: alpha
    groups: [workers]
    power:
      type: wol
      mac: "00:11:22:33:44:55"
    telemetry:
      provider: nvidia
    services: [llm]
  beta:
    site: lab
    ssh: beta
    groups: [workers]
groups:
  workers:
    devices: [beta, alpha]
services:
  llm:
    health:
      url: http://alpha:8000/health
      headers:
        Authorization: "${env:DOCTOR_TOKEN}"
"#;

struct FakeDoctor {
    local: Result<String, String>,
    devices: BTreeMap<String, DeviceProbe>,
}

impl DoctorProbe for FakeDoctor {
    fn local_ssh(&self) -> Result<String, String> {
        self.local.clone()
    }

    fn diagnose_device(
        &self,
        device_name: &str,
        _device: &Device,
        _config: &Config,
    ) -> DeviceProbe {
        self.devices[device_name].clone()
    }
}

fn healthy_probe() -> DeviceProbe {
    DeviceProbe {
        reachability: RuntimeObservation::available("configured SSH endpoint responded"),
        ssh: RuntimeObservation::available("authentication and remote command succeeded"),
        power: None,
        telemetry: None,
        capabilities: RemoteCapabilities {
            nvidia: Presence::Absent,
            unified_memory: false,
            docker: Presence::Present,
            systemd: Presence::Present,
        },
        services: Vec::new(),
    }
}

#[test]
fn doctor_selects_only_configured_target_resources_and_emits_stable_json() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let fake = FakeDoctor {
        local: Ok("/usr/bin/ssh".to_owned()),
        devices: BTreeMap::from([
            ("alpha".to_owned(), healthy_probe()),
            ("beta".to_owned(), healthy_probe()),
        ]),
    };

    let report = collect_doctor(
        &config,
        DoctorRequest {
            target: "alpha".to_owned(),
        },
        &fake,
    )
    .unwrap();
    let mut output = Vec::new();
    write_json(&mut output, &report).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(report.exit_status(), ExitStatus::Success);
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["target"], "alpha");
    assert_eq!(json["devices"].as_array().unwrap().len(), 1);
    assert_eq!(json["devices"][0]["device"], "alpha");
    assert_eq!(json["devices"][0]["checks"][0]["kind"], "reachability");
}

#[test]
fn missing_local_ssh_is_actionable_and_fails_without_device_probes() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let fake = FakeDoctor {
        local: Err("ssh executable was not found".to_owned()),
        devices: BTreeMap::new(),
    };

    let report = collect_doctor(&config, DoctorRequest::default(), &fake).unwrap();

    assert_eq!(report.exit_status(), ExitStatus::Failed);
    assert_eq!(report.local.status, CheckStatus::Fail);
    assert!(
        report
            .local
            .suggestion
            .as_deref()
            .unwrap()
            .contains("OpenSSH")
    );
    assert!(report.devices.iter().all(|device| {
        device
            .checks
            .iter()
            .all(|check| check.status == CheckStatus::Skipped)
    }));
}

#[test]
fn site_wide_unreachability_is_aggregated_without_claiming_power_state() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let unreachable = DeviceProbe {
        reachability: RuntimeObservation::unavailable("connection timed out"),
        ssh: RuntimeObservation::unknown("not attempted"),
        ..healthy_probe()
    };
    let fake = FakeDoctor {
        local: Ok("ssh".to_owned()),
        devices: BTreeMap::from([
            ("alpha".to_owned(), unreachable.clone()),
            ("beta".to_owned(), unreachable),
        ]),
    };

    let report = collect_doctor(&config, DoctorRequest::default(), &fake).unwrap();

    assert_eq!(report.exit_status(), ExitStatus::Failed);
    assert_eq!(report.sites.len(), 1);
    assert_eq!(report.sites[0].site, "lab");
    assert_eq!(report.sites[0].status, CheckStatus::Fail);
    assert!(report.sites[0].summary.contains("network/VPN"));
    assert!(
        report
            .devices
            .iter()
            .flat_map(|device| &device.checks)
            .all(|check| !check.summary.contains("powered off"))
    );
}

#[test]
fn partial_device_failures_produce_partial_exit_and_keep_all_results() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let mut unreachable = healthy_probe();
    unreachable.reachability = RuntimeObservation::unavailable("no route");
    unreachable.ssh = RuntimeObservation::unknown("not attempted");
    let fake = FakeDoctor {
        local: Ok("ssh".to_owned()),
        devices: BTreeMap::from([
            ("alpha".to_owned(), healthy_probe()),
            ("beta".to_owned(), unreachable),
        ]),
    };

    let report = collect_doctor(&config, DoctorRequest::default(), &fake).unwrap();

    assert_eq!(report.exit_status(), ExitStatus::PartialSuccess);
    assert_eq!(report.summary.devices_total, 2);
    assert_eq!(report.summary.devices_failed, 1);
    assert_eq!(report.devices.len(), 2);
}

#[test]
fn capabilities_report_uma_and_informative_missing_tools_without_failure() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let mut alpha = healthy_probe();
    alpha.telemetry = Some(Capability::available("nvidia telemetry available"));
    alpha.capabilities = RemoteCapabilities {
        nvidia: Presence::Present,
        unified_memory: true,
        docker: Presence::Absent,
        systemd: Presence::Absent,
    };
    let fake = FakeDoctor {
        local: Ok("ssh".to_owned()),
        devices: BTreeMap::from([
            ("alpha".to_owned(), alpha),
            ("beta".to_owned(), healthy_probe()),
        ]),
    };

    let report = collect_doctor(
        &config,
        DoctorRequest {
            target: "alpha".to_owned(),
        },
        &fake,
    )
    .unwrap();
    let checks = &report.devices[0].checks;

    assert_eq!(report.exit_status(), ExitStatus::Success);
    assert!(
        checks
            .iter()
            .any(|check| { check.kind.to_string() == "uma" && check.status == CheckStatus::Pass })
    );
    assert!(checks.iter().any(|check| {
        check.kind.to_string() == "docker" && check.status == CheckStatus::Warning
    }));
    assert!(checks.iter().any(|check| {
        check.kind.to_string() == "systemd" && check.status == CheckStatus::Warning
    }));
}

#[test]
fn configured_power_telemetry_and_services_are_runtime_failures() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let mut alpha = healthy_probe();
    alpha.power = Some(RuntimeObservation::unavailable("provider unreachable"));
    alpha.telemetry = Some(Capability::unsupported("nvidia-smi is unavailable"));
    alpha.services = vec![ServiceObservation {
        name: "llm".to_owned(),
        state: ServiceState::Error,
        status: None,
        health: Some(ProbeObservation {
            state: ServiceState::Error,
            http_status: None,
            error: Some("HTTP probe timed out".to_owned()),
        }),
        info: None,
        fields: BTreeMap::new(),
    }];
    let fake = FakeDoctor {
        local: Ok("ssh".to_owned()),
        devices: BTreeMap::from([("alpha".to_owned(), alpha)]),
    };

    let report = collect_doctor(
        &config,
        DoctorRequest {
            target: "alpha".to_owned(),
        },
        &fake,
    )
    .unwrap();

    assert_eq!(report.exit_status(), ExitStatus::Failed);
    assert!(report.devices[0].checks.iter().any(|check| {
        check.kind.to_string() == "power_provider" && check.status == CheckStatus::Fail
    }));
    assert!(report.devices[0].checks.iter().any(|check| {
        check.kind.to_string() == "telemetry" && check.status == CheckStatus::Warning
    }));
    assert!(
        report.devices[0].checks.iter().any(|check| {
            check.kind.to_string() == "service" && check.status == CheckStatus::Fail
        })
    );
}

#[test]
fn suggestions_and_errors_are_redacted_and_human_output_is_actionable() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let config_before = serde_yaml::to_string(&config).unwrap();
    let mut alpha = healthy_probe();
    alpha.power = Some(RuntimeObservation::unavailable(
        "token ${env:DOCTOR_TOKEN} value super-secret rejected",
    ));
    let fake = FakeDoctor {
        local: Ok("ssh".to_owned()),
        devices: BTreeMap::from([("alpha".to_owned(), alpha)]),
    };
    unsafe { std::env::set_var("DOCTOR_TOKEN", "super-secret") };

    let report = collect_doctor(
        &config,
        DoctorRequest {
            target: "alpha".to_owned(),
        },
        &fake,
    )
    .unwrap();
    let mut output = Vec::new();
    write_human(&mut output, &report).unwrap();
    let output = String::from_utf8(output).unwrap();
    unsafe { std::env::remove_var("DOCTOR_TOKEN") };

    assert!(!output.contains("super-secret"));
    assert!(!output.contains("DOCTOR_TOKEN"));
    assert!(output.contains("[REDACTED]"));
    assert!(output.contains("Suggestion:"));
    assert_eq!(serde_yaml::to_string(&config).unwrap(), config_before);
}

#[test]
fn unknown_target_is_a_usage_error() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let fake = FakeDoctor {
        local: Ok("ssh".to_owned()),
        devices: BTreeMap::new(),
    };

    let error = collect_doctor(
        &config,
        DoctorRequest {
            target: "missing".to_owned(),
        },
        &fake,
    )
    .unwrap_err();

    assert!(error.to_string().contains("unknown target"));
}
