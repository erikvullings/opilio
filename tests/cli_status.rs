use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use clap::Parser;
use opilio::{
    app,
    cli::Cli,
    domain::Device,
    status::{ExitStatus, StatusSource, StatusState},
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

static NEXT_CONFIG: AtomicUsize = AtomicUsize::new(0);

fn config_path() -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("opilio-status-tests");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!(
        "{}-{}.yaml",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, CONFIG).unwrap();
    path
}

#[test]
fn human_status_table_is_stable() {
    let path = config_path();
    let cli =
        Cli::try_parse_from(["opilio", "--config", path.to_str().unwrap(), "status"]).unwrap();
    let mut output = Vec::new();

    let status = app::execute(cli, &mut output).unwrap();

    assert_eq!(status, ExitStatus::Success);
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "\
DEVICE  SITE  SSH          STATUS      DETAIL
alpha   home  alpha.local  configured  -
beta    -     beta.local   configured  -

2 succeeded, 0 failed
"
    );
}

#[test]
fn json_status_schema_is_stable() {
    let path = config_path();
    let cli = Cli::try_parse_from([
        "opilio",
        "--config",
        path.to_str().unwrap(),
        "status",
        "home",
        "--json",
    ])
    .unwrap();
    let mut output = Vec::new();

    let status = app::execute(cli, &mut output).unwrap();

    assert_eq!(status, ExitStatus::Success);
    assert_eq!(
        String::from_utf8(output).unwrap(),
        r#"{
  "schema_version": 1,
  "target": "home",
  "summary": {
    "total": 1,
    "succeeded": 1,
    "failed": 0
  },
  "devices": [
    {
      "device": "alpha",
      "site": "home",
      "ssh": "alpha.local",
      "status": "configured",
      "error": null
    }
  ]
}
"#
    );
}

#[test]
fn status_command_resolves_device_group_site_and_default_all_targets() {
    let path = config_path();
    for (target, expected) in [
        (Some("alpha"), vec!["alpha"]),
        (Some("workers"), vec!["alpha", "beta"]),
        (Some("home"), vec!["alpha"]),
        (Some("all"), vec!["alpha", "beta"]),
        (None, vec!["alpha", "beta"]),
    ] {
        let mut arguments = vec!["opilio", "--config", path.to_str().unwrap(), "status"];
        if let Some(target) = target {
            arguments.push(target);
        }
        arguments.push("--json");
        let mut output = Vec::new();

        app::execute(Cli::try_parse_from(arguments).unwrap(), &mut output).unwrap();

        let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
        let devices = json["devices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|device| device["device"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(devices, expected);
    }
}

#[test]
fn single_device_uses_human_detail_and_quiet_suppresses_output() {
    let path = config_path();
    let mut detail = Vec::new();
    app::execute(
        Cli::try_parse_from([
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "status",
            "alpha",
        ])
        .unwrap(),
        &mut detail,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(detail).unwrap(),
        "\
Device: alpha
Site: home
SSH: alpha.local
Status: configured
Detail: -
"
    );

    let mut quiet = Vec::new();
    let status = app::execute(
        Cli::try_parse_from([
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "status",
            "--quiet",
        ])
        .unwrap(),
        &mut quiet,
    )
    .unwrap();
    assert_eq!(status, ExitStatus::Success);
    assert!(quiet.is_empty());
}

struct BetaFailure;

impl StatusSource for BetaFailure {
    fn status(&self, device_name: &str, _device: &Device) -> Result<StatusState, String> {
        if device_name == "beta" {
            Err("not reachable".to_owned())
        } else {
            Ok(StatusState::Configured)
        }
    }
}

#[test]
fn status_command_returns_partial_success_after_rendering_every_device() {
    let path = config_path();
    let cli = Cli::try_parse_from([
        "opilio",
        "--config",
        path.to_str().unwrap(),
        "status",
        "workers",
        "--json",
        "--parallel",
        "1",
    ])
    .unwrap();
    let mut output = Vec::new();

    let status = app::execute_with_status_source(cli, &mut output, &BetaFailure).unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(status, ExitStatus::PartialSuccess);
    assert_eq!(json["summary"]["succeeded"], 1);
    assert_eq!(json["summary"]["failed"], 1);
    assert_eq!(json["devices"].as_array().unwrap().len(), 2);
    assert_eq!(json["devices"][1]["error"], "not reachable");
}

#[test]
fn unknown_targets_and_invalid_parallelism_are_usage_errors() {
    let path = config_path();
    let error = app::execute(
        Cli::try_parse_from([
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "status",
            "missing",
        ])
        .unwrap(),
        &mut Vec::new(),
    )
    .unwrap_err();
    let usage = Cli::try_parse_from(["opilio", "status", "--parallel", "0"]).unwrap_err();

    assert_eq!(error.exit_code(), 2);
    assert_eq!(usage.exit_code(), 2);
}
