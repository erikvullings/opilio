use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use clap::Parser;
use opilio::{
    app,
    cli::Cli,
    config::Config,
    doctor::{DeviceProbe, DoctorProbe, Presence, RemoteCapabilities, RuntimeObservation},
    domain::Device,
    status::ExitStatus,
};

const CONFIG: &str = r#"
devices:
  alpha:
    ssh: alpha.local
    groups: [workers]
  beta:
    ssh: beta.local
    groups: [workers]
groups:
  workers:
    devices: [beta, alpha]
"#;

static NEXT_CONFIG: AtomicUsize = AtomicUsize::new(0);

fn config_path(contents: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("opilio-doctor-tests");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!(
        "{}-{}.yaml",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, contents).unwrap();
    path
}

struct FakeDoctor;

impl DoctorProbe for FakeDoctor {
    fn local_ssh(&self) -> Result<String, String> {
        Ok("/usr/bin/ssh".to_owned())
    }

    fn diagnose_device(
        &self,
        device_name: &str,
        _device: &Device,
        _config: &Config,
    ) -> DeviceProbe {
        DeviceProbe {
            reachability: if device_name == "beta" {
                RuntimeObservation::unavailable("no route")
            } else {
                RuntimeObservation::available("reachable")
            },
            ssh: if device_name == "beta" {
                RuntimeObservation::unknown("not attempted")
            } else {
                RuntimeObservation::available("connected")
            },
            power: None,
            telemetry: None,
            capabilities: RemoteCapabilities {
                nvidia: Presence::Absent,
                unified_memory: false,
                docker: Presence::Absent,
                systemd: Presence::Present,
            },
            services: Vec::new(),
        }
    }
}

#[test]
fn doctor_cli_supports_target_json_and_partial_exit() {
    let path = config_path(CONFIG);
    let cli = Cli::try_parse_from([
        "opilio",
        "--config",
        path.to_str().unwrap(),
        "doctor",
        "workers",
        "--json",
    ])
    .unwrap();
    let mut output = Vec::new();

    let status = app::execute_with_doctor_probe(cli, &mut output, &FakeDoctor).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(status, ExitStatus::PartialSuccess);
    assert_eq!(json["target"], "workers");
    assert_eq!(json["summary"]["devices_total"], 2);
    assert_eq!(json["devices"][0]["device"], "alpha");
    assert_eq!(json["devices"][1]["device"], "beta");
}

#[test]
fn doctor_cli_defaults_to_all_and_supports_quiet() {
    let path = config_path(CONFIG);
    let cli = Cli::try_parse_from([
        "opilio",
        "--config",
        path.to_str().unwrap(),
        "doctor",
        "--quiet",
    ])
    .unwrap();
    let mut output = Vec::new();

    let status = app::execute_with_doctor_probe(cli, &mut output, &FakeDoctor).unwrap();

    assert_eq!(status, ExitStatus::PartialSuccess);
    assert!(output.is_empty());
}

#[test]
fn config_check_stays_static_when_runtime_is_broken() {
    struct PanickingDoctor;
    impl DoctorProbe for PanickingDoctor {
        fn local_ssh(&self) -> Result<String, String> {
            panic!("config check performed a runtime probe")
        }

        fn diagnose_device(
            &self,
            _device_name: &str,
            _device: &Device,
            _config: &Config,
        ) -> DeviceProbe {
            panic!("config check performed a device probe")
        }
    }

    let path = config_path(CONFIG);
    let cli = Cli::try_parse_from([
        "opilio",
        "--config",
        path.to_str().unwrap(),
        "config",
        "check",
    ])
    .unwrap();
    let mut output = Vec::new();

    let status = app::execute_with_doctor_probe(cli, &mut output, &PanickingDoctor).unwrap();

    assert_eq!(status, ExitStatus::Success);
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("configuration is valid")
    );
}

#[test]
fn invalid_config_remains_a_configuration_error_before_doctor_runs() {
    let path = config_path("devices:\n  alpha:\n    ssh: alpha\n    surprise: true\n");
    let cli = Cli::try_parse_from([
        "opilio",
        "--config",
        path.to_str().unwrap(),
        "doctor",
        "--json",
    ])
    .unwrap();

    let error = app::execute_with_doctor_probe(cli, &mut Vec::new(), &FakeDoctor).unwrap_err();

    assert_eq!(error.exit_code(), 2);
    assert!(error.to_string().contains("surprise"));
}
