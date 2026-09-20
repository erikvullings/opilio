use std::{
    fs,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use clap::Parser;
use opilio::{
    app,
    cli::{Cli, Command},
    config::Config,
    doctor::{DeviceProbe, DoctorProbe, Presence, RemoteCapabilities, RuntimeObservation},
    domain::Device,
    status::ExitStatus,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn transfer_cli_exposes_stable_output_and_noninteractive_controls() {
    let export = Cli::try_parse_from([
        "opilio",
        "--config",
        "flock.yaml",
        "export",
        "flock.opilio",
        "--json",
    ])
    .unwrap();
    assert!(matches!(
        export.command,
        Some(Command::Export {
            bundle,
            json: true,
            quiet: false,
        }) if bundle == *"flock.opilio"
    ));

    let import = Cli::try_parse_from([
        "opilio",
        "--config",
        "flock.yaml",
        "import",
        "flock.opilio",
        "--non-interactive",
        "--quiet",
    ])
    .unwrap();
    assert!(matches!(
        import.command,
        Some(Command::Import {
            non_interactive: true,
            json: false,
            quiet: true,
            ..
        })
    ));
}

struct HealthyDoctor;

impl DoctorProbe for HealthyDoctor {
    fn local_ssh(&self) -> Result<String, String> {
        Ok("ssh".to_owned())
    }

    fn diagnose_device(
        &self,
        _device_name: &str,
        _device: &Device,
        _config: &Config,
    ) -> DeviceProbe {
        DeviceProbe {
            reachability: RuntimeObservation::available("reachable"),
            ssh: RuntimeObservation::available("connected"),
            power: None,
            telemetry: None,
            capabilities: RemoteCapabilities {
                nvidia: Presence::Absent,
                unified_memory: false,
                docker: Presence::Absent,
                systemd: Presence::Absent,
            },
            services: Vec::new(),
        }
    }
}

#[test]
fn transfer_cli_emits_human_and_versioned_json_reports() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("cli-transfer-tests")
        .join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".ssh")).unwrap();
    let source = root.join("source.yaml");
    let destination = root.join("destination.yaml");
    let bundle = root.join("flock.opilio");
    fs::write(&source, "devices:\n  alpha:\n    ssh: alpha\n").unwrap();
    fs::write(
        root.join(".ssh/config"),
        "Host alpha\n  HostName alpha.example\n",
    )
    .unwrap();
    let previous_home = std::env::var_os("HOME");
    unsafe { std::env::set_var("HOME", &root) };

    let export = Cli::try_parse_from([
        "opilio",
        "--config",
        source.to_str().unwrap(),
        "export",
        bundle.to_str().unwrap(),
    ])
    .unwrap();
    let mut human = Vec::new();
    assert_eq!(
        app::execute_with_doctor_probe(export, &mut human, &HealthyDoctor).unwrap(),
        ExitStatus::Success
    );
    assert!(
        String::from_utf8(human)
            .unwrap()
            .contains("exported 1 SSH host")
    );

    let import = Cli::try_parse_from([
        "opilio",
        "--config",
        destination.to_str().unwrap(),
        "import",
        bundle.to_str().unwrap(),
        "--non-interactive",
        "--json",
    ])
    .unwrap();
    let mut json = Vec::new();
    assert_eq!(
        app::execute_with_doctor_probe(import, &mut json, &HealthyDoctor).unwrap(),
        ExitStatus::Success
    );
    let report: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["bundle_version"], 1);
    assert_eq!(report["validation"], "configuration is valid");
    assert_eq!(report["diagnostics"]["target"], "all");

    match previous_home {
        Some(value) => unsafe { std::env::set_var("HOME", value) },
        None => unsafe { std::env::remove_var("HOME") },
    }
}
