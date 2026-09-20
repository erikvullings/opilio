use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use clap::Parser;
use opilio::{
    action::ActionExecutor,
    app,
    cli::Cli,
    history::{HistoryConfig, HistoryResult, HistoryStore, OperationSource, Redactor},
    ssh::{ExecutionOptions, ProcessOutput, RemoteInvocation},
    status::ExitStatus,
};

static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

fn fixture() -> (PathBuf, HistoryStore) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("opilio-cli-history-tests")
        .join(format!(
            "{}-{}",
            std::process::id(),
            NEXT_CASE.fetch_add(1, Ordering::Relaxed)
        ));
    fs::create_dir_all(&root).unwrap();
    let config = root.join("config.yaml");
    fs::write(
        &config,
        r#"
devices:
  alpha:
    ssh: alpha-host
    groups: [workers]
  beta:
    ssh: beta-host
    groups: [workers]
groups:
  workers:
    devices: [alpha, beta]
actions:
  update:
    command: update
"#,
    )
    .unwrap();
    let history = HistoryStore::with_redactor(
        HistoryConfig {
            directory: root.join("history"),
            max_file_bytes: 1024 * 1024,
            max_files: 3,
            failure_tail_bytes: 64,
        },
        Redactor::new(["known-secret".to_owned()]),
    )
    .unwrap();
    (config, history)
}

struct FakeAction;

impl ActionExecutor for FakeAction {
    fn execute(
        &self,
        ssh_target: &str,
        _invocation: &RemoteInvocation,
        _options: ExecutionOptions,
    ) -> Result<ProcessOutput, String> {
        Ok(if ssh_target == "alpha-host" {
            ProcessOutput {
                exit_code: Some(0),
                stdout: b"successful output must not persist".to_vec(),
                ..ProcessOutput::default()
            }
        } else {
            ProcessOutput {
                exit_code: Some(17),
                stdout: b"failed stdout known-secret".to_vec(),
                stderr: b"failed stderr known-secret".to_vec(),
                ..ProcessOutput::default()
            }
        })
    }
}

#[test]
fn action_execution_is_recorded_centrally_and_history_cli_is_stable() {
    let (config, history) = fixture();
    let status = app::execute_with_action_executor_and_history(
        Cli::try_parse_from([
            "opilio",
            "--config",
            config.to_str().unwrap(),
            "action",
            "run",
            "update",
            "workers",
            "--quiet",
        ])
        .unwrap(),
        &mut Vec::new(),
        &FakeAction,
        OperationSource::Cli,
        &history,
    )
    .unwrap();
    assert_eq!(status, ExitStatus::PartialSuccess);

    let records = history.list(None).unwrap().records;
    assert_eq!(records.len(), 2);
    let success = records
        .iter()
        .find(|record| record.resolved_device == "alpha")
        .unwrap();
    assert_eq!(success.operation, "action");
    assert_eq!(success.action.as_deref(), Some("update"));
    assert_eq!(success.requested_target, "workers");
    assert_eq!(success.result, HistoryResult::Succeeded);
    assert_eq!(success.exit_code, Some(0));
    assert_eq!(success.stdout, None);
    assert_eq!(success.stderr, None);
    let failure = records
        .iter()
        .find(|record| record.resolved_device == "beta")
        .unwrap();
    assert_eq!(failure.result, HistoryResult::Failed);
    assert_eq!(failure.exit_code, Some(17));
    assert_eq!(failure.stdout.as_deref(), Some("failed stdout [REDACTED]"));
    assert_eq!(failure.stderr.as_deref(), Some("failed stderr [REDACTED]"));

    let mut filtered = Vec::new();
    app::execute_with_history(
        Cli::try_parse_from([
            "opilio",
            "--config",
            config.to_str().unwrap(),
            "history",
            "alpha",
            "--json",
        ])
        .unwrap(),
        &mut filtered,
        OperationSource::Cli,
        &history,
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&filtered).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["records"].as_array().unwrap().len(), 1);
    assert_eq!(json["records"][0]["resolved_device"], "alpha");

    let mut human = Vec::new();
    app::execute_with_history(
        Cli::try_parse_from([
            "opilio",
            "--config",
            config.to_str().unwrap(),
            "history",
            "alpha",
        ])
        .unwrap(),
        &mut human,
        OperationSource::Cli,
        &history,
    )
    .unwrap();
    let human = String::from_utf8(human).unwrap();
    assert!(human.contains(&success.id));
    assert!(human.contains("cli action alpha succeeded"));

    let mut shown = Vec::new();
    app::execute_with_history(
        Cli::try_parse_from([
            "opilio",
            "--config",
            config.to_str().unwrap(),
            "history",
            "show",
            &failure.id,
            "--json",
        ])
        .unwrap(),
        &mut shown,
        OperationSource::Cli,
        &history,
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&shown).unwrap();
    assert_eq!(json["record"]["id"], failure.id);
    assert_eq!(json["warnings"], serde_json::json!([]));
}

#[test]
fn caller_source_supports_cli_tui_and_scheduled_operations() {
    for (value, expected) in [
        ("cli", OperationSource::Cli),
        ("tui", OperationSource::Tui),
        ("scheduled", OperationSource::Scheduled),
    ] {
        let cli = Cli::try_parse_from(["opilio", "--source", value, "history"]).unwrap();
        assert_eq!(cli.source, expected);
    }
}
