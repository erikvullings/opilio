use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use opilio::{
    config::{Config, ConfigPathOptions, Platform, SecretRef},
    target::Target,
};

const VALID_CONFIG: &str = r#"
sites:
  home:
    label: Home
devices:
  alpha:
    site: home
    ssh: alpha
    groups: [workers]
    power:
      type: shelly
      host: shelly-alpha
      auth:
        password: "${env:SHELLY_PASSWORD}"
    shutdown:
      command: sudo shutdown -h now
      cut_power: true
      timeout: 2m
    telemetry:
      provider: nvidia
    services: [llm]
  beta:
    site: home
    ssh: beta
    groups: [workers]
    power:
      type: wol
      mac: "00:11:22:33:44:55"
groups:
  workers:
    devices: [alpha, beta]
actions:
  update:
    command: sudo apt update
    timeout: 30m
  start:
    cwd: /srv/app
    exec:
      program: ./start
      args: []
    timeout: 2m
    overrides:
      workers:
        command: systemctl start app
      alpha:
        command: docker start app
aliases:
  stop-workers:
    operation: off
    target: workers
    parallel: 2
services:
  llm:
    status:
      command: docker inspect llm
    health:
      url: http://localhost:8000/health
    info:
      url: http://localhost:8000/v1/models
"#;

#[test]
fn valid_portable_config_loads_and_resolves_references() {
    let config = Config::from_yaml(VALID_CONFIG).expect("valid config");

    assert_eq!(config.devices().len(), 2);
    assert_eq!(
        config
            .resolve_action("start", "alpha")
            .expect("device override")
            .command(),
        Some("docker start app")
    );
    assert_eq!(
        config
            .resolve_action("start", "beta")
            .expect("group override")
            .command(),
        Some("systemctl start app")
    );
    assert_eq!(
        config.resolve_action("start", "beta").unwrap().cwd(),
        Some("/srv/app")
    );
    assert_eq!(
        config.resolve_action("start", "beta").unwrap().timeout(),
        Some(Duration::from_secs(120))
    );
    assert_eq!(
        config
            .resolve_action("update", "alpha")
            .expect("default")
            .command(),
        Some("sudo apt update")
    );
}

#[test]
fn config_path_precedence_is_explicit_then_environment_then_platform_default() {
    let environment = BTreeMap::from([
        ("OPILIO_CONFIG".into(), "/env/config.yaml".into()),
        ("XDG_CONFIG_HOME".into(), "/xdg".into()),
        ("HOME".into(), "/home/alice".into()),
        ("APPDATA".into(), r"C:\Users\Alice\AppData\Roaming".into()),
    ]);

    let explicit = ConfigPathOptions::new(
        Some(PathBuf::from("/explicit/config.yaml")),
        Platform::Linux,
        &environment,
    );
    assert_eq!(
        explicit.resolve().unwrap(),
        Path::new("/explicit/config.yaml")
    );

    let from_environment = ConfigPathOptions::new(None, Platform::Linux, &environment);
    assert_eq!(
        from_environment.resolve().unwrap(),
        Path::new("/env/config.yaml")
    );

    let mut defaults = environment;
    defaults.remove("OPILIO_CONFIG");
    assert_eq!(
        ConfigPathOptions::new(None, Platform::Linux, &defaults)
            .resolve()
            .unwrap(),
        Path::new("/xdg/opilio/config.yaml")
    );
    assert_eq!(
        ConfigPathOptions::new(None, Platform::MacOs, &defaults)
            .resolve()
            .unwrap(),
        Path::new("/home/alice/.config/opilio/config.yaml")
    );
    assert_eq!(
        ConfigPathOptions::new(None, Platform::Windows, &defaults)
            .resolve()
            .unwrap(),
        PathBuf::from(r"C:\Users\Alice\AppData\Roaming")
            .join("opilio")
            .join("config.yaml")
    );
}

#[test]
fn target_resolution_supports_device_group_site_and_all() {
    let config = Config::from_yaml(VALID_CONFIG).unwrap();

    assert_eq!(Target::resolve("alpha", &config).unwrap(), ["alpha"]);
    assert_eq!(
        Target::resolve("workers", &config).unwrap(),
        ["alpha", "beta"]
    );
    assert_eq!(Target::resolve("home", &config).unwrap(), ["alpha", "beta"]);
    assert_eq!(Target::resolve("all", &config).unwrap(), ["alpha", "beta"]);
}

#[test]
fn conflicting_group_overrides_are_rejected_regardless_of_input_order() {
    let yaml = VALID_CONFIG
        .replace("groups: [workers]", "groups: [gpu, workers]")
        .replace(
            "groups:\n  workers:\n    devices: [alpha, beta]",
            "groups:\n  gpu:\n    devices: [alpha, beta]\n  workers:\n    devices: [beta, alpha]",
        )
        .replace(
            "workers:\n        command: systemctl start app",
            "workers:\n        command: systemctl start app\n      gpu:\n        command: docker compose up",
        )
        .replace(
            "      alpha:\n        command: docker start app\n",
            "",
        );

    let error = Config::from_yaml(&yaml).unwrap_err().to_string();
    assert!(error.contains("start"));
    assert!(error.contains("alpha"));
    assert!(error.contains("gpu"));
    assert!(error.contains("workers"));
}

#[test]
fn device_override_resolves_overlapping_group_overrides() {
    let yaml = VALID_CONFIG
        .replacen("groups: [workers]", "groups: [gpu, workers]", 1)
        .replace(
            "groups:\n  workers:\n    devices: [alpha, beta]",
            "groups:\n  gpu:\n    devices: [alpha]\n  workers:\n    devices: [alpha, beta]",
        )
        .replace(
            "workers:\n        command: systemctl start app",
            "workers:\n        command: systemctl start app\n      gpu:\n        command: docker compose up",
        );

    let config = Config::from_yaml(&yaml).unwrap();
    assert_eq!(
        config.resolve_action("start", "alpha").unwrap().command(),
        Some("docker start app")
    );
}

#[test]
fn unknown_and_conflicting_references_have_actionable_diagnostics() {
    let unknown = VALID_CONFIG.replace("site: home", "site: missing");
    assert!(
        Config::from_yaml(&unknown)
            .unwrap_err()
            .to_string()
            .contains("unknown site `missing`")
    );

    let conflicting = VALID_CONFIG.replace("groups: [workers]", "groups: []");
    assert!(
        Config::from_yaml(&conflicting)
            .unwrap_err()
            .to_string()
            .contains("membership")
    );
}

#[test]
fn duplicate_membership_is_rejected() {
    let duplicate = VALID_CONFIG.replace("devices: [alpha, beta]", "devices: [alpha, beta, alpha]");

    let error = Config::from_yaml(&duplicate).unwrap_err().to_string();

    assert!(error.contains("duplicate `alpha`"));
    assert!(error.contains("membership"));
}

#[test]
fn command_and_exec_are_mutually_exclusive_and_unknown_fields_fail() {
    let both = VALID_CONFIG.replace(
        "command: sudo apt update\n    timeout:",
        "command: sudo apt update\n    exec:\n      program: apt\n      args: [update]\n    timeout:",
    );
    assert!(
        Config::from_yaml(&both)
            .unwrap_err()
            .to_string()
            .contains("both `command` and `exec`")
    );

    let unknown = VALID_CONFIG.replace("ssh: alpha", "ssh: alpha\n    mystery: true");
    assert!(
        Config::from_yaml(&unknown)
            .unwrap_err()
            .to_string()
            .contains("mystery")
    );
}

#[test]
fn malformed_provider_settings_are_rejected() {
    let malformed_wol =
        VALID_CONFIG.replace("mac: \"00:11:22:33:44:55\"", "mac: \"not-a-mac-address\"");
    assert!(
        Config::from_yaml(&malformed_wol)
            .unwrap_err()
            .to_string()
            .contains("MAC")
    );

    let malformed_shelly = VALID_CONFIG.replace("host: shelly-alpha", "host: \"\"");
    assert!(
        Config::from_yaml(&malformed_shelly)
            .unwrap_err()
            .to_string()
            .contains("Shelly host")
    );
}

#[test]
fn secret_references_and_values_are_redacted() {
    let reference: SecretRef = "${env:API_TOKEN}".parse().unwrap();
    let secret = reference
        .resolve_with(|name| (name == "API_TOKEN").then(|| "super-secret".to_owned()))
        .unwrap();

    assert_eq!(reference.environment_variable(), "API_TOKEN");
    assert_eq!(format!("{reference:?}"), "${env:API_TOKEN}");
    assert_eq!(format!("{secret}"), "[REDACTED]");
    assert_eq!(format!("{secret:?}"), "[REDACTED]");
    assert!(!format!("{secret:?}").contains("super-secret"));
}

#[test]
fn invalid_alias_service_and_override_references_fail() {
    let bad_alias = VALID_CONFIG.replace("target: workers", "target: missing");
    assert!(
        Config::from_yaml(&bad_alias)
            .unwrap_err()
            .to_string()
            .contains("alias `stop-workers`")
    );

    let bad_service = VALID_CONFIG.replace("services: [llm]", "services: [missing]");
    assert!(
        Config::from_yaml(&bad_service)
            .unwrap_err()
            .to_string()
            .contains("unknown service `missing`")
    );

    let bad_override = VALID_CONFIG.replace(
        "workers:\n        command: systemctl start app",
        "missing:\n        command: systemctl start app",
    );
    assert!(
        Config::from_yaml(&bad_override)
            .unwrap_err()
            .to_string()
            .contains("override target `missing`")
    );
}
