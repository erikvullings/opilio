use std::{
    fs,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use clap::Parser;
use opilio::{
    app::{self, Confirmation},
    cli::Cli,
    lifecycle::{LifecycleExecutor, LifecycleOperation, PlannedStep},
    status::ExitStatus,
};

const CONFIG: &str = r#"
devices:
  alpha:
    ssh: alpha-host
    groups: [fleet]
    power:
      type: shelly
      host: alpha-plug
    shutdown:
      command: sudo shutdown -h now
      cut_power: true
      timeout: 1s
  beta:
    ssh: beta-host
    groups: [fleet]
    power:
      type: wol
      mac: "00:11:22:33:44:55"
groups:
  fleet:
    devices: [beta, alpha]
aliases:
  restart-fleet:
    operation: reboot
    target: fleet
    parallel: 2
"#;

static NEXT_CONFIG: AtomicUsize = AtomicUsize::new(0);

fn config_path() -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("opilio-lifecycle-tests");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!(
        "{}-{}.yaml",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, CONFIG).unwrap();
    path
}

#[derive(Default)]
struct FakeExecutor {
    calls: Mutex<Vec<(String, PlannedStep)>>,
}

impl LifecycleExecutor for FakeExecutor {
    fn execute_step(
        &self,
        device_name: &str,
        _device: &opilio::domain::Device,
        step: PlannedStep,
        _timeout: Duration,
    ) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push((device_name.to_owned(), step));
        if device_name == "beta" && step == PlannedStep::GracefulShutdown {
            Err("SSH unavailable".to_owned())
        } else {
            Ok(())
        }
    }
}

struct FakeConfirmation {
    answer: bool,
    prompts: Mutex<Vec<(LifecycleOperation, Vec<String>)>>,
}

impl FakeConfirmation {
    fn accepting() -> Self {
        Self {
            answer: true,
            prompts: Mutex::new(Vec::new()),
        }
    }
}

impl Confirmation for FakeConfirmation {
    fn confirm(&self, operation: LifecycleOperation, devices: &[String]) -> Result<bool, String> {
        self.prompts
            .lock()
            .unwrap()
            .push((operation, devices.to_vec()));
        Ok(self.answer)
    }
}

#[test]
fn group_operation_confirms_with_resolved_devices_then_emits_partial_json() {
    let path = config_path();
    let executor = FakeExecutor::default();
    let confirmation = FakeConfirmation::accepting();
    let cli = Cli::try_parse_from([
        "opilio",
        "--config",
        path.to_str().unwrap(),
        "shutdown",
        "fleet",
        "--json",
    ])
    .unwrap();
    let mut output = Vec::new();

    let status =
        app::execute_with_lifecycle_executor(cli, &mut output, &executor, &confirmation).unwrap();

    assert_eq!(status, ExitStatus::PartialSuccess);
    assert_eq!(
        confirmation.prompts.into_inner().unwrap(),
        vec![(
            LifecycleOperation::Shutdown,
            vec!["alpha".to_owned(), "beta".to_owned()]
        )]
    );
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["summary"]["succeeded"], 1);
    assert_eq!(json["summary"]["failed"], 1);
    assert_eq!(json["devices"][1]["error"], "SSH unavailable");
}

#[test]
fn yes_skips_confirmation_but_does_not_grant_force() {
    let path = config_path();
    let executor = FakeExecutor::default();
    let confirmation = FakeConfirmation::accepting();
    let cli = Cli::try_parse_from([
        "opilio",
        "--config",
        path.to_str().unwrap(),
        "power-off",
        "alpha",
        "--yes",
    ])
    .unwrap();

    let error =
        app::execute_with_lifecycle_executor(cli, &mut Vec::new(), &executor, &confirmation)
            .unwrap_err();

    assert_eq!(error.exit_code(), 2);
    assert!(error.to_string().contains("requires `--force`"));
    assert!(executor.calls.lock().unwrap().is_empty());
}

#[test]
fn single_device_routine_operation_never_prompts() {
    let path = config_path();
    let executor = FakeExecutor::default();
    let confirmation = FakeConfirmation {
        answer: false,
        prompts: Mutex::new(Vec::new()),
    };
    let cli = Cli::try_parse_from([
        "opilio",
        "--config",
        path.to_str().unwrap(),
        "on",
        "alpha",
        "--wait",
    ])
    .unwrap();
    let mut output = Vec::new();

    let status =
        app::execute_with_lifecycle_executor(cli, &mut output, &executor, &confirmation).unwrap();

    assert_eq!(status, ExitStatus::Success);
    assert!(confirmation.prompts.lock().unwrap().is_empty());
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "alpha: ssh-ready\n1 succeeded, 0 failed\n"
    );
}

#[test]
fn declining_collection_confirmation_prevents_every_device_operation() {
    let path = config_path();
    let executor = FakeExecutor::default();
    let confirmation = FakeConfirmation {
        answer: false,
        prompts: Mutex::new(Vec::new()),
    };
    let cli = Cli::try_parse_from([
        "opilio",
        "--config",
        path.to_str().unwrap(),
        "reboot",
        "fleet",
    ])
    .unwrap();

    let error =
        app::execute_with_lifecycle_executor(cli, &mut Vec::new(), &executor, &confirmation)
            .unwrap_err();

    assert_eq!(error.exit_code(), 1);
    assert!(executor.calls.lock().unwrap().is_empty());
}

#[test]
fn lifecycle_aliases_use_the_same_confirmation_and_execution_path() {
    let path = config_path();
    let executor = FakeExecutor::default();
    let confirmation = FakeConfirmation::accepting();
    let cli = Cli::try_parse_from([
        "opilio",
        "--config",
        path.to_str().unwrap(),
        "alias",
        "run",
        "restart-fleet",
        "--quiet",
    ])
    .unwrap();

    let status =
        app::execute_with_lifecycle_executor(cli, &mut Vec::new(), &executor, &confirmation)
            .unwrap();

    assert_eq!(status, ExitStatus::Success);
    assert_eq!(executor.calls.lock().unwrap().len(), 2);
    assert_eq!(confirmation.prompts.lock().unwrap().len(), 1);
}

#[test]
fn scheduled_collection_commands_and_aliases_are_inherently_confirmed() {
    for command in [
        vec!["reboot", "fleet", "--quiet"],
        vec!["alias", "run", "restart-fleet", "--quiet"],
    ] {
        let path = config_path();
        let executor = FakeExecutor::default();
        let confirmation = FakeConfirmation {
            answer: false,
            prompts: Mutex::new(Vec::new()),
        };
        let mut args = vec![
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "--source",
            "scheduled",
        ];
        args.extend(command);

        let status = app::execute_with_lifecycle_executor(
            Cli::try_parse_from(args).unwrap(),
            &mut Vec::new(),
            &executor,
            &confirmation,
        )
        .unwrap();

        assert_eq!(status, ExitStatus::Success);
        assert_eq!(executor.calls.lock().unwrap().len(), 2);
        assert!(confirmation.prompts.lock().unwrap().is_empty());
    }
}
