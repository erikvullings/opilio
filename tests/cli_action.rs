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
    action::ActionExecutor,
    app,
    cli::Cli,
    ssh::{ExecutionOptions, ProcessOutput, RemoteInvocation},
    status::ExitStatus,
};

const CONFIG: &str = r#"
devices:
  alpha:
    ssh: alpha-host
    groups: [workers]
  beta:
    ssh: beta-host
    groups: [workers]
groups:
  workers:
    devices: [beta, alpha]
actions:
  update:
    command: sudo update
  version:
    exec:
      program: uname
      args: [-a]
aliases:
  fleet-status:
    operation: status
    target: workers
    parallel: 2
"#;

static NEXT_CONFIG: AtomicUsize = AtomicUsize::new(0);

fn config_path() -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("opilio-action-tests");
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
    calls: Mutex<Vec<String>>,
}

impl ActionExecutor for FakeExecutor {
    fn execute(
        &self,
        ssh_target: &str,
        _invocation: &RemoteInvocation,
        _options: ExecutionOptions,
    ) -> Result<ProcessOutput, String> {
        self.calls.lock().unwrap().push(ssh_target.to_owned());
        Ok(if ssh_target == "beta-host" {
            ProcessOutput {
                exit_code: Some(9),
                stderr: b"failed".to_vec(),
                ..ProcessOutput::default()
            }
        } else {
            ProcessOutput {
                exit_code: Some(0),
                stdout: b"updated".to_vec(),
                ..ProcessOutput::default()
            }
        })
    }
}

#[test]
fn action_list_supports_stable_human_and_json_output() {
    let path = config_path();
    let mut human = Vec::new();
    app::execute(
        Cli::try_parse_from(["opilio", "--config", path.to_str().unwrap(), "action", "ls"])
            .unwrap(),
        &mut human,
    )
    .unwrap();
    assert_eq!(String::from_utf8(human).unwrap(), "update\nversion\n");

    let mut json = Vec::new();
    app::execute(
        Cli::try_parse_from([
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "action",
            "ls",
            "--json",
        ])
        .unwrap(),
        &mut json,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(json).unwrap(),
        "{\n  \"schema_version\": 1,\n  \"actions\": [\n    \"update\",\n    \"version\"\n  ]\n}\n"
    );
}

#[test]
fn action_run_json_aggregates_every_device_and_returns_partial_success() {
    let path = config_path();
    let executor = FakeExecutor::default();
    let cli = Cli::try_parse_from([
        "opilio",
        "--config",
        path.to_str().unwrap(),
        "action",
        "run",
        "update",
        "workers",
        "--json",
        "--parallel",
        "2",
    ])
    .unwrap();
    let mut output = Vec::new();

    let status = app::execute_with_action_executor(cli, &mut output, &executor).unwrap();

    assert_eq!(status, ExitStatus::PartialSuccess);
    assert_eq!(
        String::from_utf8(output.clone()).unwrap(),
        r#"{
  "schema_version": 1,
  "action": "update",
  "target": "workers",
  "summary": {
    "total": 2,
    "succeeded": 1,
    "failed": 1
  },
  "devices": [
    {
      "device": "alpha",
      "site": null,
      "ssh": "alpha-host",
      "status": "succeeded",
      "exit_code": 0,
      "timed_out": false,
      "cancelled": false,
      "stdout": "updated",
      "stderr": "",
      "error": null
    },
    {
      "device": "beta",
      "site": null,
      "ssh": "beta-host",
      "status": "failed",
      "exit_code": 9,
      "timed_out": false,
      "cancelled": false,
      "stdout": "",
      "stderr": "failed",
      "error": "remote action exited with code 9"
    }
  ]
}
"#
    );
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["action"], "update");
    assert_eq!(json["target"], "workers");
    assert_eq!(json["summary"]["succeeded"], 1);
    assert_eq!(json["summary"]["failed"], 1);
    assert_eq!(json["devices"][0]["device"], "alpha");
    assert_eq!(json["devices"][1]["device"], "beta");
    assert_eq!(json["devices"][1]["exit_code"], 9);
}

#[test]
fn action_run_has_concise_human_output_and_quiet_mode() {
    let path = config_path();
    let executor = FakeExecutor::default();
    let mut human = Vec::new();

    let status = app::execute_with_action_executor(
        Cli::try_parse_from([
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "action",
            "run",
            "version",
            "alpha",
        ])
        .unwrap(),
        &mut human,
        &executor,
    )
    .unwrap();
    assert_eq!(status, ExitStatus::Success);
    assert_eq!(
        String::from_utf8(human).unwrap(),
        "alpha: succeeded\n1 succeeded, 0 failed\n"
    );

    let mut quiet = Vec::new();
    app::execute_with_action_executor(
        Cli::try_parse_from([
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "action",
            "run",
            "version",
            "alpha",
            "--quiet",
        ])
        .unwrap(),
        &mut quiet,
        &executor,
    )
    .unwrap();
    assert!(quiet.is_empty());
}

#[test]
fn alias_expands_once_to_status_with_its_fixed_target_and_options() {
    let path = config_path();
    let mut output = Vec::new();
    let status = app::execute(
        Cli::try_parse_from([
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "alias",
            "run",
            "fleet-status",
            "--json",
        ])
        .unwrap(),
        &mut output,
    )
    .unwrap();

    assert_eq!(status, ExitStatus::Success);
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["target"], "workers");
    assert_eq!(json["summary"]["total"], 2);
}

#[test]
fn unknown_alias_and_recursive_alias_shape_are_rejected_as_usage_or_config() {
    let path = config_path();
    let error = app::execute(
        Cli::try_parse_from([
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "alias",
            "run",
            "missing",
        ])
        .unwrap(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert_eq!(error.exit_code(), 2);

    let recursive = CONFIG.replace("operation: status", "operation: alias");
    fs::write(&path, recursive).unwrap();
    let error = app::execute(
        Cli::try_parse_from([
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "config",
            "check",
        ])
        .unwrap(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert_eq!(error.exit_code(), 2);
}

#[test]
fn unknown_actions_targets_and_zero_parallelism_are_usage_errors() {
    let path = config_path();
    for arguments in [
        vec!["action", "run", "missing", "all"],
        vec!["action", "run", "update", "missing"],
    ] {
        let mut command = vec!["opilio", "--config", path.to_str().unwrap()];
        command.extend(arguments);
        let error = app::execute_with_action_executor(
            Cli::try_parse_from(command).unwrap(),
            &mut Vec::new(),
            &FakeExecutor::default(),
        )
        .unwrap_err();
        assert_eq!(error.exit_code(), 2);
    }

    let usage = Cli::try_parse_from([
        "opilio",
        "action",
        "run",
        "update",
        "all",
        "--parallel",
        "0",
    ])
    .unwrap_err();
    assert_eq!(usage.exit_code(), 2);
}
