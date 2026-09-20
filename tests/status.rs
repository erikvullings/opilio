use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{
        Barrier,
        atomic::{AtomicUsize, Ordering},
    },
};

use opilio::{
    config::Config,
    domain::Device,
    service::{ProbeObservation, ServiceObservation, ServiceState},
    status::{
        ConcurrentExecutor, ConfiguredStatusSource, ExitStatus, StatusRequest, StatusSource,
        StatusState, collect_status, write_human,
    },
    telemetry::{
        Metric, ProviderSnapshot, TelemetrySnapshot,
        system::{LoadAverage, SystemMemory, SystemMetrics},
    },
};

const CONFIG: &str = r#"
sites:
  home:
    label: Home
devices:
  alpha:
    site: home
    ssh: alpha.local
    groups: [workers]
  beta:
    ssh: beta.local
    groups: [workers]
groups:
  workers:
    devices: [beta, alpha]
"#;

#[test]
fn status_defaults_to_all_configured_devices() {
    let config = Config::from_yaml(CONFIG).unwrap();

    let report =
        collect_status(&config, StatusRequest::default(), &ConfiguredStatusSource).unwrap();

    assert_eq!(report.target, "all");
    assert_eq!(report.summary.total, 2);
    assert_eq!(report.summary.succeeded, 2);
    assert_eq!(report.summary.failed, 0);
    assert_eq!(report.exit_status(), ExitStatus::Success);
    assert_eq!(
        report
            .devices
            .iter()
            .map(|result| result.device.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
}

struct OneFailure;

impl StatusSource for OneFailure {
    fn status(&self, device_name: &str, _device: &Device) -> Result<StatusState, String> {
        if device_name == "alpha" {
            Err("probe failed".to_owned())
        } else {
            Ok(StatusState::Configured)
        }
    }
}

#[test]
fn group_status_retains_every_device_and_reports_partial_success() {
    let config = Config::from_yaml(CONFIG).unwrap();

    let report = collect_status(
        &config,
        StatusRequest {
            target: "workers".to_owned(),
            ..StatusRequest::default()
        },
        &OneFailure,
    )
    .unwrap();

    assert_eq!(report.summary.succeeded, 1);
    assert_eq!(report.summary.failed, 1);
    assert_eq!(report.devices.len(), 2);
    assert_eq!(report.devices[0].error.as_deref(), Some("probe failed"));
    assert_eq!(report.devices[1].status, StatusState::Configured);
    assert_eq!(report.exit_status(), ExitStatus::PartialSuccess);
}

#[test]
fn concurrent_executor_respects_its_parallelism_limit() {
    let active = AtomicUsize::new(0);
    let maximum = AtomicUsize::new(0);
    let barrier = Barrier::new(2);
    let executor = ConcurrentExecutor::new(NonZeroUsize::new(2).unwrap());

    let results = executor.run(&[1, 2, 3, 4], |item| {
        let now_active = active.fetch_add(1, Ordering::SeqCst) + 1;
        maximum.fetch_max(now_active, Ordering::SeqCst);
        barrier.wait();
        active.fetch_sub(1, Ordering::SeqCst);
        item * 2
    });

    assert_eq!(results, [2, 4, 6, 8]);
    assert_eq!(maximum.load(Ordering::SeqCst), 2);
}

struct AllFail;

impl StatusSource for AllFail {
    fn status(&self, _device_name: &str, _device: &Device) -> Result<StatusState, String> {
        Err("unavailable".to_owned())
    }
}

#[test]
fn exit_status_codes_cover_success_failure_usage_and_partial_success() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let failed = collect_status(&config, StatusRequest::default(), &AllFail).unwrap();
    let unknown = collect_status(
        &config,
        StatusRequest {
            target: "missing".to_owned(),
            ..StatusRequest::default()
        },
        &ConfiguredStatusSource,
    )
    .unwrap_err();

    assert_eq!(ExitStatus::Success.code(), 0);
    assert_eq!(failed.exit_status(), ExitStatus::Failed);
    assert_eq!(failed.exit_status().code(), 1);
    assert_eq!(ExitStatus::ConfigOrUsage.code(), 2);
    assert_eq!(
        unknown.to_string(),
        "unknown target `missing`; expected a device, group, site, or `all`"
    );
    assert_eq!(ExitStatus::PartialSuccess.code(), 3);
}

struct TelemetryFixture;

impl StatusSource for TelemetryFixture {
    fn status(&self, _device_name: &str, _device: &Device) -> Result<StatusState, String> {
        Ok(StatusState::Configured)
    }

    fn telemetry(&self, _device_name: &str, device: &Device) -> Option<TelemetrySnapshot> {
        device.telemetry.as_ref()?;
        Some(TelemetrySnapshot {
            collected_at_unix_ms: 1_000,
            system: ProviderSnapshot::Available {
                data: SystemMetrics {
                    cpu_busy_percent: Metric::available(12.5),
                    logical_cpus: Metric::available(4),
                    load: LoadAverage {
                        one: Metric::available(0.5),
                        five: Metric::available(0.4),
                        fifteen: Metric::available(0.3),
                    },
                    memory: SystemMemory {
                        total_bytes: Metric::available(1024),
                        available_bytes: Metric::available(512),
                    },
                    uptime_seconds: Metric::available(60),
                },
            },
            nvidia: None,
        })
    }
}

#[test]
fn configured_telemetry_is_exposed_without_changing_schema_for_other_devices() {
    let config = Config::from_yaml(&CONFIG.replace(
        "    site: home",
        "    site: home\n    telemetry:\n      provider: system",
    ))
    .unwrap();

    let report = collect_status(&config, StatusRequest::default(), &TelemetryFixture).unwrap();
    let json = serde_json::to_value(&report).unwrap();

    assert_eq!(
        json["devices"][0]["telemetry"]["system"]["state"],
        "available"
    );
    assert_eq!(
        json["devices"][0]["telemetry"]["system"]["data"]["cpu_busy_percent"],
        serde_json::json!({"state": "available", "value": 12.5})
    );
    assert!(json["devices"][1].get("telemetry").is_none());
}

struct ServiceFixture;

impl StatusSource for ServiceFixture {
    fn status(&self, _device_name: &str, _device: &Device) -> Result<StatusState, String> {
        Ok(StatusState::Configured)
    }

    fn services(
        &self,
        _device_name: &str,
        device: &Device,
        _config: &Config,
    ) -> Option<Vec<ServiceObservation>> {
        (!device.services.is_empty()).then(|| {
            vec![ServiceObservation {
                name: "llm".to_owned(),
                state: ServiceState::Ready,
                status: None,
                health: Some(ProbeObservation {
                    state: ServiceState::Ready,
                    http_status: Some(200),
                    error: None,
                }),
                info: None,
                fields: BTreeMap::from([("model".to_owned(), serde_json::json!("Qwen/Qwen3-32B"))]),
            }]
        })
    }
}

#[test]
fn configured_services_are_an_additive_status_json_surface() {
    let config = Config::from_yaml(
        &(CONFIG.replace("    site: home", "    site: home\n    services: [llm]")
            + "\nservices:\n  llm:\n    health:\n      url: http://localhost/health\n"),
    )
    .unwrap();
    let report = collect_status(&config, StatusRequest::default(), &ServiceFixture).unwrap();
    let json = serde_json::to_value(&report).unwrap();

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["devices"][0]["services"][0]["state"], "ready");
    assert_eq!(
        json["devices"][0]["services"][0]["fields"]["model"],
        "Qwen/Qwen3-32B"
    );
    assert!(json["devices"][1].get("services").is_none());

    let mut human = Vec::new();
    write_human(&mut human, &report).unwrap();
    let human = String::from_utf8(human).unwrap();
    assert!(human.contains("llm=ready (model=Qwen/Qwen3-32B)"));
}
