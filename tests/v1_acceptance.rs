use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use clap::{CommandFactory, Parser, error::ErrorKind};
use opilio::{
    action::ActionExecutor,
    app::{self, Confirmation},
    cli::Cli,
    domain::Device,
    lifecycle::{LifecycleExecutor, LifecycleOperation, PlannedStep},
    scheduler::{AtTime, ScheduleCommand, ScheduleId, platform},
    ssh::{ExecutionOptions, InteractiveSsh, ProcessOutput, RemoteInvocation, SshError},
    status::{ExitStatus, StatusSource, StatusState},
};

const CONFIG: &str = r#"
sites:
  home:
    label: Home
devices:
  alpha:
    site: home
    ssh: alpha-alias
    power:
      type: shelly
      host: alpha-plug
    shutdown:
      command: sudo shutdown -h now
      cut_power: true
actions:
  inspect:
    exec:
      program: uname
      args: [-a]
"#;

static NEXT_CONFIG: AtomicUsize = AtomicUsize::new(0);

fn config_path() -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("opilio-v1-acceptance");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!(
        "{}-{}.yaml",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, CONFIG).unwrap();
    path
}

fn cli(path: &Path, arguments: &[&str]) -> Cli {
    let mut argv = vec!["opilio", "--config", path.to_str().unwrap()];
    argv.extend_from_slice(arguments);
    Cli::try_parse_from(argv).unwrap()
}

#[derive(Default)]
struct FakeFleet {
    lifecycle: Mutex<Vec<PlannedStep>>,
    actions: Mutex<Vec<String>>,
    ssh: Mutex<Vec<String>>,
}

impl StatusSource for FakeFleet {
    fn status(&self, _device_name: &str, _device: &Device) -> Result<StatusState, String> {
        Ok(StatusState::Configured)
    }
}

impl LifecycleExecutor for FakeFleet {
    fn execute_step(
        &self,
        _device_name: &str,
        _device: &Device,
        step: PlannedStep,
        _timeout: Duration,
    ) -> Result<(), String> {
        self.lifecycle.lock().unwrap().push(step);
        Ok(())
    }
}

impl ActionExecutor for FakeFleet {
    fn execute(
        &self,
        ssh_target: &str,
        _invocation: &RemoteInvocation,
        _options: ExecutionOptions,
    ) -> Result<ProcessOutput, String> {
        self.actions.lock().unwrap().push(ssh_target.to_owned());
        Ok(ProcessOutput {
            exit_code: Some(0),
            stdout: b"Linux alpha".to_vec(),
            ..ProcessOutput::default()
        })
    }
}

impl InteractiveSsh for FakeFleet {
    fn interactive(&self, target: &str) -> Result<i32, SshError> {
        self.ssh.lock().unwrap().push(target.to_owned());
        Ok(0)
    }
}

struct RejectUnexpectedPrompt;

impl Confirmation for RejectUnexpectedPrompt {
    fn confirm(&self, _operation: LifecycleOperation, _devices: &[String]) -> Result<bool, String> {
        panic!("single-device acceptance operations must not prompt")
    }
}

#[test]
fn configured_device_supports_status_power_action_and_ssh_without_hardware() {
    let path = config_path();
    let fleet = FakeFleet::default();
    let mut status_json = Vec::new();
    assert_eq!(
        app::execute_with_status_source(
            cli(&path, &["status", "alpha", "--json"]),
            &mut status_json,
            &fleet,
        )
        .unwrap(),
        ExitStatus::Success
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&status_json).unwrap()["devices"][0]["device"],
        "alpha"
    );

    for operation in [["on", "alpha"], ["off", "alpha"]] {
        assert_eq!(
            app::execute_with_lifecycle_executor(
                cli(&path, &operation),
                &mut Vec::new(),
                &fleet,
                &RejectUnexpectedPrompt,
            )
            .unwrap(),
            ExitStatus::Success
        );
    }
    assert_eq!(
        *fleet.lifecycle.lock().unwrap(),
        [
            PlannedStep::RequestPowerOn,
            PlannedStep::GracefulShutdown,
            PlannedStep::WaitForShutdown,
            PlannedStep::CutPhysicalPower,
        ]
    );

    assert_eq!(
        app::execute_with_action_executor(
            cli(&path, &["action", "run", "inspect", "alpha", "--quiet"]),
            &mut Vec::new(),
            &fleet,
        )
        .unwrap(),
        ExitStatus::Success
    );
    assert_eq!(*fleet.actions.lock().unwrap(), ["alpha-alias"]);

    assert_eq!(
        app::execute_with_ssh(cli(&path, &["ssh", "alpha"]), &mut Vec::new(), &fleet,).unwrap(),
        ExitStatus::Success
    );
    assert_eq!(*fleet.ssh.lock().unwrap(), ["alpha-alias"]);
}

#[test]
fn all_native_scheduler_artifacts_are_generated_without_host_mutation() {
    let id = ScheduleId::new("daily-status").unwrap();
    let at = "08:05".parse::<AtTime>().unwrap();
    let command = ScheduleCommand::new(vec!["status".into(), "all".into()]).unwrap();
    let executable = Path::new("C:/Program Files/Opilio/opilio.exe");
    let config = Path::new("C:/Users/Test User/.config/opilio/config.yaml");

    let systemd = platform::systemd(Path::new("/units"), &id, at, executable, config, &command);
    let launchd = platform::launchd(
        Path::new("/agents"),
        501,
        &id,
        at,
        executable,
        config,
        &command,
    );
    let windows = platform::windows(Path::new("C:/state"), &id, at, executable, config, &command);

    for plan in [systemd, launchd, windows] {
        assert!(!plan.artifacts.is_empty());
        assert!(
            plan.artifacts
                .iter()
                .all(|artifact| artifact.contents.contains("OPILIO_SCHEDULE_V1"))
        );
        assert!(!plan.install.is_empty());
        assert!(!plan.remove.is_empty());
    }
}

#[test]
fn help_and_usage_errors_follow_cli_exit_contract() {
    Cli::command().debug_assert();
    let help = Cli::try_parse_from(["opilio", "--help"]).unwrap_err();
    let usage = Cli::try_parse_from(["opilio", "ssh"]).unwrap_err();

    assert_eq!(help.kind(), ErrorKind::DisplayHelp);
    assert_eq!(help.exit_code(), 0);
    assert_eq!(usage.kind(), ErrorKind::MissingRequiredArgument);
    assert_eq!(usage.exit_code(), 2);
}

struct FailingStatus(&'static str);

impl StatusSource for FailingStatus {
    fn status(&self, _device_name: &str, _device: &Device) -> Result<StatusState, String> {
        Err(format!("probe exposed {}", self.0))
    }
}

#[test]
fn status_json_redacts_resolved_controller_secrets() {
    const VARIABLE: &str = "OPILIO_TEST_STATUS_SECRET";
    const SECRET: &str = "task-0016-status-secret";
    let path = config_path();
    fs::write(
        &path,
        CONFIG.replace(
            "actions:\n",
            &format!(
                "services:\n  protected:\n    health:\n      url: http://localhost/health\n      headers:\n        Authorization: \"${{env:{VARIABLE}}}\"\nactions:\n"
            ),
        ),
    )
    .unwrap();
    unsafe { std::env::set_var(VARIABLE, SECRET) };
    let mut output = Vec::new();

    let status = app::execute_with_status_source(
        cli(&path, &["status", "alpha", "--json"]),
        &mut output,
        &FailingStatus(SECRET),
    )
    .unwrap();

    unsafe { std::env::remove_var(VARIABLE) };
    let output = String::from_utf8(output).unwrap();
    assert_eq!(status, ExitStatus::Failed);
    assert!(!output.contains(SECRET));
    assert!(output.contains("[REDACTED]"));
}

struct FailingLifecycle(&'static str);

impl LifecycleExecutor for FailingLifecycle {
    fn execute_step(
        &self,
        _device_name: &str,
        _device: &Device,
        _step: PlannedStep,
        _timeout: Duration,
    ) -> Result<(), String> {
        Err(format!("provider exposed {}", self.0))
    }
}

#[test]
fn lifecycle_json_redacts_resolved_controller_secrets() {
    const VARIABLE: &str = "OPILIO_TEST_LIFECYCLE_SECRET";
    const SECRET: &str = "task-0016-lifecycle-secret";
    let path = config_path();
    fs::write(
        &path,
        CONFIG.replace(
            "actions:\n",
            &format!(
                "services:\n  protected:\n    health:\n      url: http://localhost/health\n      headers:\n        Authorization: \"${{env:{VARIABLE}}}\"\nactions:\n"
            ),
        ),
    )
    .unwrap();
    unsafe { std::env::set_var(VARIABLE, SECRET) };
    let mut output = Vec::new();

    let status = app::execute_with_lifecycle_executor(
        cli(&path, &["on", "alpha", "--json"]),
        &mut output,
        &FailingLifecycle(SECRET),
        &RejectUnexpectedPrompt,
    )
    .unwrap();

    unsafe { std::env::remove_var(VARIABLE) };
    let output = String::from_utf8(output).unwrap();
    assert_eq!(status, ExitStatus::Failed);
    assert!(!output.contains(SECRET));
    assert!(output.contains("[REDACTED]"));
}
