use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicUsize},
};

use clap::Parser;
use opilio::{
    app,
    cli::Cli,
    ssh::{InteractiveSsh, SshError},
    status::ExitStatus,
};

const CONFIG: &str = r#"
sites:
  home:
    label: Home
devices:
  alpha:
    site: home
    ssh: alpha-ssh-alias
    groups: [workers]
  beta:
    site: home
    ssh: beta-ssh-alias
    groups: [workers]
groups:
  workers:
    devices: [alpha, beta]
"#;

static NEXT_CONFIG: AtomicUsize = AtomicUsize::new(0);

fn config_path() -> PathBuf {
    use std::sync::atomic::Ordering;

    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("opilio-ssh-tests");
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
struct FakeSsh {
    targets: Mutex<Vec<String>>,
}

impl InteractiveSsh for FakeSsh {
    fn interactive(&self, target: &str) -> Result<i32, SshError> {
        self.targets.lock().unwrap().push(target.to_owned());
        Ok(0)
    }
}

#[test]
fn ssh_resolves_one_device_and_hands_off_its_openssh_alias() {
    let ssh = Arc::new(FakeSsh::default());
    let path = config_path();
    let cli = Cli::try_parse_from(["opilio", "--config", path.to_str().unwrap(), "ssh", "alpha"])
        .unwrap();

    let status = app::execute_with_ssh(cli, &mut Vec::new(), ssh.as_ref()).unwrap();

    assert_eq!(status, ExitStatus::Success);
    assert_eq!(*ssh.targets.lock().unwrap(), ["alpha-ssh-alias"]);
}

#[test]
fn ssh_rejects_groups_sites_and_unknown_targets_before_process_execution() {
    let path = config_path();
    for target in ["workers", "home", "missing"] {
        let ssh = FakeSsh::default();
        let cli =
            Cli::try_parse_from(["opilio", "--config", path.to_str().unwrap(), "ssh", target])
                .unwrap();

        let error = app::execute_with_ssh(cli, &mut Vec::new(), &ssh).unwrap_err();

        assert_eq!(error.exit_code(), 2);
        assert!(ssh.targets.lock().unwrap().is_empty());
    }
}
