use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use clap::Parser;
use opilio::{app, cli::Cli};

const SAMPLE: &str = include_str!("../examples/config.yaml");
static NEXT_CONFIG: AtomicUsize = AtomicUsize::new(0);

fn sample_path() -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("opilio-config-tests");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!(
        "{}-{}.yaml",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, SAMPLE).unwrap();
    path
}

#[test]
fn config_check_and_path_use_the_explicit_config_without_runtime_probes() {
    let path = sample_path();

    let mut check_output = Vec::new();
    app::execute(
        Cli::try_parse_from([
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "config",
            "check",
        ])
        .unwrap(),
        &mut check_output,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(check_output).unwrap(),
        format!("configuration is valid: {}\n", path.display())
    );

    let mut path_output = Vec::new();
    app::execute(
        Cli::try_parse_from([
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "config",
            "path",
        ])
        .unwrap(),
        &mut path_output,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(path_output).unwrap(),
        format!("{}\n", path.display())
    );
}

#[test]
fn list_commands_load_validated_config_and_emit_stable_sorted_names() {
    let path = sample_path();
    for (noun, expected) in [
        ("device", "spark-2\nspark-home\n"),
        ("group", "sglang\nsparks\n"),
        ("site", "home\nlab\n"),
        ("action", "start-llm\nupdate\n"),
    ] {
        let cli = Cli::try_parse_from(["opilio", "--config", path.to_str().unwrap(), noun, "ls"])
            .unwrap();
        let mut output = Vec::new();

        app::execute(cli, &mut output).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), expected);
    }
}

#[test]
fn config_and_name_reads_have_versioned_json_and_quiet_output() {
    let path = sample_path();
    let mut path_json = Vec::new();
    app::execute(
        Cli::try_parse_from([
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "config",
            "path",
            "--json",
        ])
        .unwrap(),
        &mut path_json,
    )
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&path_json).unwrap(),
        serde_json::json!({"schema_version": 1, "path": path})
    );

    let mut check_json = Vec::new();
    app::execute(
        Cli::try_parse_from([
            "opilio",
            "--config",
            path.to_str().unwrap(),
            "config",
            "check",
            "--json",
        ])
        .unwrap(),
        &mut check_json,
    )
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&check_json).unwrap(),
        serde_json::json!({"schema_version": 1, "path": path, "valid": true})
    );

    for (noun, names) in [
        ("device", serde_json::json!(["spark-2", "spark-home"])),
        ("group", serde_json::json!(["sglang", "sparks"])),
        ("site", serde_json::json!(["home", "lab"])),
    ] {
        let mut json = Vec::new();
        app::execute(
            Cli::try_parse_from([
                "opilio",
                "--config",
                path.to_str().unwrap(),
                noun,
                "ls",
                "--json",
            ])
            .unwrap(),
            &mut json,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&json).unwrap(),
            serde_json::json!({
                "schema_version": 1,
                "kind": noun,
                "names": names
            })
        );

        let mut quiet = Vec::new();
        app::execute(
            Cli::try_parse_from([
                "opilio",
                "--config",
                path.to_str().unwrap(),
                noun,
                "ls",
                "--quiet",
            ])
            .unwrap(),
            &mut quiet,
        )
        .unwrap();
        assert!(quiet.is_empty());
    }
}

#[test]
fn executable_read_dispatch_ignores_invalid_history_settings() {
    let path = sample_path();
    for arguments in [
        vec!["config", "path", "--quiet"],
        vec!["config", "check", "--quiet"],
        vec!["device", "ls", "--quiet"],
        vec!["group", "ls", "--quiet"],
        vec!["site", "ls", "--quiet"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_opilio"))
            .arg("--config")
            .arg(&path)
            .args(arguments)
            .env("OPILIO_HISTORY_MAX_FILES", "invalid")
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn config_check_rejects_invalid_configuration() {
    let path = sample_path();
    fs::write(&path, format!("{SAMPLE}\nunknown_root: true\n")).unwrap();
    let cli = Cli::try_parse_from([
        "opilio",
        "--config",
        path.to_str().unwrap(),
        "config",
        "check",
    ])
    .unwrap();

    let error = app::execute(cli, &mut Vec::new()).unwrap_err().to_string();

    assert!(error.contains("unknown_root"));
}
