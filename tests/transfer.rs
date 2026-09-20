use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

static TEST_LOCK: Mutex<()> = Mutex::new(());

use opilio::config::Platform;
use opilio::{
    config::Config,
    doctor::{DeviceProbe, DoctorProbe, Presence, RemoteCapabilities, RuntimeObservation},
    domain::Device,
    status::ExitStatus,
    transfer::{
        ExportRequest, ImportInteraction, ImportMappings, ImportRequest, LocalRequirements,
        SshPathOptions, export_bundle, import_bundle,
    },
};

const CONFIG: &str = r#"
devices:
  alpha:
    ssh: alpha
    power:
      type: shelly
      host: plug
      auth:
        password: "${env:OPILIO_PASSWORD}"
  beta:
    ssh: beta
"#;

const SSH_CONFIG: &str = r#"
Host unrelated
  HostName unrelated.example
  IdentityFile ~/.ssh/unrelated-private

Host alpha
  HostName alpha.example
  User old-user
  IdentityFile ~/.ssh/id_alpha
  ProxyJump jump-one

Host beta
  HostName beta.example
  User old-user
  IdentityFile C:\Users\old\.ssh\id_beta
  ProxyJump jump-one

Host jump-one
  HostName jump.example
  User jump-user
  IdentityFile ~/.ssh/id_jump
  ProxyJump jump-two

Host jump-two
  HostName jump-two.example
  IdentityFile ~/.ssh/id_jump_two
"#;

fn root(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("transfer-tests")
        .join(format!("{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn ssh_paths_follow_linux_macos_and_windows_controller_conventions() {
    let _guard = TEST_LOCK.lock().unwrap();
    let environment = [
        ("HOME".to_owned(), "/Users/controller".to_owned()),
        ("USERPROFILE".to_owned(), r"C:\Users\controller".to_owned()),
    ]
    .into_iter()
    .collect();

    for platform in [Platform::Linux, Platform::MacOs] {
        let (user, owned) = SshPathOptions::new(platform, &environment)
            .resolve()
            .unwrap();
        assert_eq!(user, PathBuf::from("/Users/controller/.ssh/config"));
        assert_eq!(owned, PathBuf::from("/Users/controller/.ssh/opilio/config"));
    }
    let (user, owned) = SshPathOptions::new(Platform::Windows, &environment)
        .resolve()
        .unwrap();
    assert_eq!(
        user,
        PathBuf::from(r"C:\Users\controller").join(".ssh/config")
    );
    assert_eq!(
        owned,
        PathBuf::from(r"C:\Users\controller").join(".ssh/opilio/config")
    );
}

#[test]
fn export_is_selective_recursive_and_never_contains_secrets_or_private_keys() {
    let _guard = TEST_LOCK.lock().unwrap();
    let root = root("export");
    let config = root.join("config.yaml");
    let ssh = root.join("ssh-config");
    let bundle = root.join("flock.opilio");
    fs::write(&config, CONFIG).unwrap();
    fs::write(&ssh, SSH_CONFIG).unwrap();
    unsafe { std::env::set_var("OPILIO_PASSWORD", "resolved-top-secret") };
    fs::write(root.join("id_alpha"), "PRIVATE KEY MATERIAL").unwrap();

    let report = export_bundle(&ExportRequest {
        config_path: config,
        ssh_config_path: ssh,
        bundle_path: bundle.clone(),
    })
    .unwrap();
    unsafe { std::env::remove_var("OPILIO_PASSWORD") };
    let bytes = fs::read(bundle).unwrap();
    let mut archive = tar::Archive::new(Cursor::new(bytes));
    let mut contents = String::new();
    for entry in archive.entries().unwrap() {
        use std::io::Read;
        entry.unwrap().read_to_string(&mut contents).unwrap();
    }

    assert_eq!(
        report.ssh_hosts,
        vec!["alpha", "beta", "jump-one", "jump-two"]
    );
    assert!(contents.contains("Host jump-two"));
    assert!(!contents.contains("Host unrelated"));
    assert!(!contents.contains("resolved-top-secret"));
    assert!(!contents.contains("PRIVATE KEY MATERIAL"));
    assert!(!contents.contains("IdentityFile"));
    assert!(contents.contains("OPILIO_PASSWORD"));
}

#[derive(Clone)]
struct MappingInteraction {
    calls: Arc<Mutex<usize>>,
}

impl ImportInteraction for MappingInteraction {
    fn mappings(&self, requirements: &LocalRequirements) -> Result<ImportMappings, String> {
        *self.calls.lock().unwrap() += 1;
        assert_eq!(requirements.identity_files.len(), 4);
        Ok(ImportMappings {
            username: Some("new-user".to_owned()),
            identity_file: Some(PathBuf::from("/Users/new/.ssh/id_ed25519")),
        })
    }
}

struct HealthyDoctor {
    calls: Arc<Mutex<Vec<String>>>,
    config_path: PathBuf,
}

impl DoctorProbe for HealthyDoctor {
    fn local_ssh(&self) -> Result<String, String> {
        assert!(Config::load(&self.config_path).is_ok());
        self.calls.lock().unwrap().push("doctor".to_owned());
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

fn export_fixture(root: &Path) -> PathBuf {
    let config = root.join("source.yaml");
    let ssh = root.join("source-ssh");
    let bundle = root.join("flock.opilio");
    fs::write(&config, CONFIG).unwrap();
    fs::write(&ssh, SSH_CONFIG).unwrap();
    export_bundle(&ExportRequest {
        config_path: config,
        ssh_config_path: ssh,
        bundle_path: bundle.clone(),
    })
    .unwrap();
    bundle
}

#[test]
fn interactive_import_maps_once_writes_owned_include_then_validates_and_diagnoses() {
    let _guard = TEST_LOCK.lock().unwrap();
    let root = root("interactive");
    let bundle = export_fixture(&root);
    let destination = root.join("config/config.yaml");
    let user_ssh = root.join(".ssh/config");
    let owned_ssh = root.join(".ssh/opilio/config");
    let calls = Arc::new(Mutex::new(0));
    let diagnostics = Arc::new(Mutex::new(Vec::new()));

    let report = import_bundle(
        &ImportRequest {
            bundle_path: bundle,
            config_path: destination.clone(),
            user_ssh_config_path: user_ssh.clone(),
            owned_ssh_config_path: owned_ssh.clone(),
            non_interactive: false,
        },
        &MappingInteraction {
            calls: Arc::clone(&calls),
        },
        &HealthyDoctor {
            calls: Arc::clone(&diagnostics),
            config_path: destination.clone(),
        },
    )
    .unwrap();

    assert_eq!(*calls.lock().unwrap(), 1);
    assert_eq!(&*diagnostics.lock().unwrap(), &["doctor"]);
    assert!(Config::load(&destination).is_ok());
    let original_config = Config::from_yaml(CONFIG).unwrap();
    let imported_config = Config::load(&destination).unwrap();
    assert_eq!(
        serde_yaml::to_string(&original_config).unwrap(),
        serde_yaml::to_string(&imported_config).unwrap()
    );
    let imported_ssh = fs::read_to_string(owned_ssh).unwrap();
    assert_eq!(imported_ssh.matches("User new-user").count(), 3);
    assert_eq!(
        imported_ssh
            .matches("IdentityFile /Users/new/.ssh/id_ed25519")
            .count(),
        4
    );
    assert_eq!(
        fs::read_to_string(&user_ssh)
            .unwrap()
            .matches("Include opilio/config")
            .count(),
        1
    );
    assert_eq!(report.exit_status(), ExitStatus::PartialSuccess);
    assert_eq!(report.unresolved.len(), 1);

    import_bundle(
        &ImportRequest {
            bundle_path: root.join("flock.opilio"),
            config_path: destination,
            user_ssh_config_path: user_ssh.clone(),
            owned_ssh_config_path: root.join(".ssh/opilio/config"),
            non_interactive: false,
        },
        &MappingInteraction { calls },
        &HealthyDoctor {
            calls: diagnostics,
            config_path: root.join("config/config.yaml"),
        },
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(user_ssh)
            .unwrap()
            .matches("Include opilio/config")
            .count(),
        1
    );
}

struct NoInteraction;

impl ImportInteraction for NoInteraction {
    fn mappings(&self, _requirements: &LocalRequirements) -> Result<ImportMappings, String> {
        panic!("non-interactive import prompted")
    }
}

#[test]
fn noninteractive_import_is_safe_and_reports_unresolved_local_setup() {
    let _guard = TEST_LOCK.lock().unwrap();
    let root = root("noninteractive");
    let bundle = export_fixture(&root);
    let destination = root.join("windows/AppData/opilio/config.yaml");
    let report = import_bundle(
        &ImportRequest {
            bundle_path: bundle,
            config_path: destination.clone(),
            user_ssh_config_path: root.join("windows/.ssh/config"),
            owned_ssh_config_path: root.join("windows/.ssh/opilio/config"),
            non_interactive: true,
        },
        &NoInteraction,
        &HealthyDoctor {
            calls: Arc::new(Mutex::new(Vec::new())),
            config_path: destination,
        },
    )
    .unwrap();

    assert_eq!(report.exit_status(), ExitStatus::PartialSuccess);
    assert!(
        report
            .unresolved
            .iter()
            .any(|item| item.contains("SSH identity"))
    );
    assert!(
        report
            .unresolved
            .iter()
            .any(|item| item.contains("OPILIO_PASSWORD"))
    );
}

#[test]
fn rejects_unknown_version_malformed_and_traversal_members_without_writing() {
    let _guard = TEST_LOCK.lock().unwrap();
    let root = root("invalid");
    let destination = root.join("config.yaml");
    let malformed = root.join("malformed.opilio");
    fs::write(&malformed, "not a tar archive").unwrap();

    let request = |bundle_path| ImportRequest {
        bundle_path,
        config_path: destination.clone(),
        user_ssh_config_path: root.join("ssh/config"),
        owned_ssh_config_path: root.join("ssh/opilio/config"),
        non_interactive: true,
    };
    assert!(
        import_bundle(
            &request(malformed),
            &NoInteraction,
            &HealthyDoctor {
                calls: Arc::new(Mutex::new(Vec::new())),
                config_path: destination.clone(),
            }
        )
        .is_err()
    );

    let version = root.join("version.opilio");
    write_archive(&version, &[("manifest.yaml", "bundle_version: 99\n")]);
    let error = import_bundle(
        &request(version),
        &NoInteraction,
        &HealthyDoctor {
            calls: Arc::new(Mutex::new(Vec::new())),
            config_path: destination.clone(),
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("bundle version"));

    let traversal = root.join("traversal.opilio");
    let mut bytes = archive_bytes("manifest.yaml", "bundle_version: 1\n");
    bytes[0..100].fill(0);
    bytes[0..11].copy_from_slice(b"../evil.txt");
    fix_tar_checksum(&mut bytes[0..512]);
    fs::write(&traversal, bytes).unwrap();
    let error = import_bundle(
        &request(traversal),
        &NoInteraction,
        &HealthyDoctor {
            calls: Arc::new(Mutex::new(Vec::new())),
            config_path: destination.clone(),
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("unsafe bundle path"));
    assert!(!destination.exists());
}

struct PanickingDoctor;

impl DoctorProbe for PanickingDoctor {
    fn local_ssh(&self) -> Result<String, String> {
        panic!("doctor ran before static validation")
    }

    fn diagnose_device(
        &self,
        _device_name: &str,
        _device: &Device,
        _config: &Config,
    ) -> DeviceProbe {
        panic!("doctor ran before static validation")
    }
}

#[test]
fn static_validation_precedes_writes_and_targeted_diagnostics() {
    let _guard = TEST_LOCK.lock().unwrap();
    let root = root("validation-order");
    let bundle = root.join("invalid.opilio");
    write_archive(
        &bundle,
        &[
            (
                "manifest.yaml",
                "bundle_version: 1\nflock: flock.yaml\nopenssh: ssh_config\nrequired_environment: []\nssh:\n  hosts: []\n  usernames: []\n  identities: []\n",
            ),
            (
                "flock.yaml",
                "devices:\n  alpha:\n    ssh: alpha\n    unknown: true\n",
            ),
            ("ssh_config", "# no entries\n"),
        ],
    );
    let destination = root.join("config.yaml");

    let error = import_bundle(
        &ImportRequest {
            bundle_path: bundle,
            config_path: destination.clone(),
            user_ssh_config_path: root.join(".ssh/config"),
            owned_ssh_config_path: root.join(".ssh/opilio/config"),
            non_interactive: true,
        },
        &NoInteraction,
        &PanickingDoctor,
    )
    .unwrap_err();

    assert!(error.to_string().contains("unknown"));
    assert!(!destination.exists());
    assert!(!root.join(".ssh/opilio/config").exists());
}

fn write_archive(path: &Path, entries: &[(&str, &str)]) {
    let file = fs::File::create(path).unwrap();
    let mut builder = tar::Builder::new(file);
    for (name, contents) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        builder
            .append_data(&mut header, *name, Cursor::new(contents.as_bytes()))
            .unwrap();
    }
    builder.finish().unwrap();
}

fn archive_bytes(name: &str, contents: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut bytes);
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        builder
            .append_data(&mut header, name, Cursor::new(contents.as_bytes()))
            .unwrap();
        builder.finish().unwrap();
    }
    bytes
}

fn fix_tar_checksum(header: &mut [u8]) {
    header[148..156].fill(b' ');
    let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    let encoded = format!("{checksum:06o}\0 ");
    header[148..156].copy_from_slice(encoded.as_bytes());
}
