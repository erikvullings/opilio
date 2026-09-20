use std::{
    fs,
    path::PathBuf,
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
