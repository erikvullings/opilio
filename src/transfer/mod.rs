//! Safe, versioned migration bundles for portable flock configuration.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    env, fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    config::{Config, ConfigError, Platform},
    doctor::{DoctorError, DoctorProbe, DoctorReport, DoctorRequest, collect_doctor},
    status::ExitStatus,
};

const BUNDLE_VERSION: u32 = 1;
const MANIFEST_PATH: &str = "manifest.yaml";
const FLOCK_PATH: &str = "flock.yaml";
const SSH_PATH: &str = "ssh_config";
const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ExportRequest {
    pub config_path: PathBuf,
    pub ssh_config_path: PathBuf,
    pub bundle_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportReport {
    pub schema_version: u32,
    pub bundle: PathBuf,
    pub ssh_hosts: Vec<String>,
    pub required_environment: Vec<String>,
    pub identity_files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ImportRequest {
    pub bundle_path: PathBuf,
    pub config_path: PathBuf,
    pub user_ssh_config_path: PathBuf,
    pub owned_ssh_config_path: PathBuf,
    pub non_interactive: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportMappings {
    pub username: Option<String>,
    pub identity_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalRequirements {
    pub environment_variables: Vec<String>,
    pub ssh_usernames: Vec<String>,
    pub identity_files: Vec<String>,
}

pub trait ImportInteraction {
    /// Prompts for all controller-local mappings once.
    fn mappings(&self, requirements: &LocalRequirements) -> Result<ImportMappings, String>;
}

pub struct SshPathOptions<'a> {
    platform: Platform,
    environment: &'a BTreeMap<String, String>,
}

impl<'a> SshPathOptions<'a> {
    pub fn new(platform: Platform, environment: &'a BTreeMap<String, String>) -> Self {
        Self {
            platform,
            environment,
        }
    }

    pub fn resolve(&self) -> Result<(PathBuf, PathBuf), TransferError> {
        let variable = match self.platform {
            Platform::Linux | Platform::MacOs => "HOME",
            Platform::Windows => "USERPROFILE",
        };
        let home = self
            .environment
            .get(variable)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                TransferError::Bundle(format!("cannot determine SSH paths: {variable} is not set"))
            })?;
        let ssh = PathBuf::from(home).join(".ssh");
        Ok((ssh.join("config"), ssh.join("opilio").join("config")))
    }
}

/// Returns the user's OpenSSH config and the isolated Opilio-owned include path.
pub fn default_ssh_paths() -> Result<(PathBuf, PathBuf), TransferError> {
    let environment = env::vars().collect::<BTreeMap<_, _>>();
    SshPathOptions::new(Platform::current(), &environment).resolve()
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub schema_version: u32,
    pub bundle_version: u32,
    pub config_path: PathBuf,
    pub ssh_config_path: PathBuf,
    pub imported_ssh_hosts: Vec<String>,
    pub unresolved: Vec<String>,
    pub validation: String,
    pub diagnostics: DoctorReport,
}

impl ImportReport {
    pub fn exit_status(&self) -> ExitStatus {
        match self.diagnostics.exit_status() {
            ExitStatus::Failed => ExitStatus::Failed,
            ExitStatus::PartialSuccess => ExitStatus::PartialSuccess,
            ExitStatus::ConfigOrUsage => ExitStatus::ConfigOrUsage,
            ExitStatus::Success if self.unresolved.is_empty() => ExitStatus::Success,
            ExitStatus::Success => ExitStatus::PartialSuccess,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleManifest {
    bundle_version: u32,
    flock: String,
    openssh: String,
    required_environment: Vec<String>,
    ssh: SshMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SshMetadata {
    hosts: Vec<String>,
    usernames: Vec<String>,
    identities: Vec<IdentityRequirement>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityRequirement {
    label: String,
    hosts: Vec<String>,
}

#[derive(Debug, Clone)]
struct HostBlock {
    hosts: Vec<String>,
    directives: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("cannot read `{path}`: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("cannot write `{path}`: {source}")]
    Write { path: PathBuf, source: io::Error },
    #[error("invalid OpenSSH configuration: {0}")]
    SshConfig(String),
    #[error("invalid transfer bundle: {0}")]
    Bundle(String),
    #[error("unsupported bundle version {found}; expected {expected}")]
    Version { found: u32, expected: u32 },
    #[error("configuration validation failed: {0}")]
    Config(#[from] ConfigError),
    #[error("diagnostics failed: {0}")]
    Doctor(#[from] DoctorError),
    #[error("interactive import failed: {0}")]
    Interaction(String),
    #[error("cannot serialize transfer data: {0}")]
    Serialize(String),
}

pub fn export_bundle(request: &ExportRequest) -> Result<ExportReport, TransferError> {
    if request.bundle_path == request.config_path || request.bundle_path == request.ssh_config_path
    {
        return Err(TransferError::Bundle(
            "bundle destination must not overwrite an input file".to_owned(),
        ));
    }
    let config_yaml = read_string(&request.config_path)?;
    let config = Config::from_yaml(&config_yaml)?;
    let ssh_source = match fs::read_to_string(&request.ssh_config_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(TransferError::Read {
                path: request.ssh_config_path.clone(),
                source,
            });
        }
    };
    let selected = select_ssh_entries(
        &ssh_source,
        config.devices().values().map(|device| device.ssh.as_str()),
    )?;
    let required_environment = config.required_secret_names();
    let manifest = BundleManifest {
        bundle_version: BUNDLE_VERSION,
        flock: FLOCK_PATH.to_owned(),
        openssh: SSH_PATH.to_owned(),
        required_environment: required_environment.clone(),
        ssh: selected.metadata,
    };
    let manifest_yaml = serde_yaml::to_string(&manifest)
        .map_err(|error| TransferError::Serialize(error.to_string()))?;
    let portable_yaml = serde_yaml::to_string(&config)
        .map_err(|error| TransferError::Serialize(error.to_string()))?;
    for secret in config
        .resolved_secret_values()
        .into_iter()
        .filter(|secret| !secret.is_empty())
    {
        if selected.config.contains(&secret)
            || manifest_yaml.contains(&secret)
            || portable_yaml.contains(&secret)
        {
            return Err(TransferError::Bundle(
                "refusing to export content containing a resolved secret value".to_owned(),
            ));
        }
    }

    let mut bytes = Vec::new();
    {
        let mut archive = tar::Builder::new(&mut bytes);
        append_archive_file(&mut archive, MANIFEST_PATH, manifest_yaml.as_bytes())?;
        append_archive_file(&mut archive, FLOCK_PATH, portable_yaml.as_bytes())?;
        append_archive_file(&mut archive, SSH_PATH, selected.config.as_bytes())?;
        archive
            .finish()
            .map_err(|error| TransferError::Bundle(error.to_string()))?;
    }
    atomic_write(&request.bundle_path, &bytes)?;

    Ok(ExportReport {
        schema_version: 1,
        bundle: request.bundle_path.clone(),
        ssh_hosts: manifest.ssh.hosts,
        required_environment,
        identity_files: manifest
            .ssh
            .identities
            .into_iter()
            .map(|identity| identity.label)
            .collect(),
    })
}

pub fn import_bundle(
    request: &ImportRequest,
    interaction: &dyn ImportInteraction,
    doctor: &dyn DoctorProbe,
) -> Result<ImportReport, TransferError> {
    let destinations = [
        &request.config_path,
        &request.user_ssh_config_path,
        &request.owned_ssh_config_path,
    ];
    if destinations
        .iter()
        .enumerate()
        .any(|(index, path)| destinations[index + 1..].contains(path))
        || destinations.contains(&&request.bundle_path)
    {
        return Err(TransferError::Bundle(
            "bundle and import destination paths must be distinct".to_owned(),
        ));
    }
    let files = read_archive(&request.bundle_path)?;
    let manifest_yaml = required_file(&files, MANIFEST_PATH)?;
    let version_document: serde_yaml::Value = serde_yaml::from_slice(manifest_yaml)
        .map_err(|error| TransferError::Bundle(format!("malformed manifest: {error}")))?;
    let found_version = version_document
        .get("bundle_version")
        .and_then(serde_yaml::Value::as_u64)
        .ok_or_else(|| {
            TransferError::Bundle("manifest is missing numeric `bundle_version`".to_owned())
        })? as u32;
    if found_version != BUNDLE_VERSION {
        return Err(TransferError::Version {
            found: found_version,
            expected: BUNDLE_VERSION,
        });
    }
    let manifest: BundleManifest = serde_yaml::from_slice(manifest_yaml)
        .map_err(|error| TransferError::Bundle(format!("malformed manifest: {error}")))?;
    validate_manifest_paths(&manifest)?;
    let flock = required_file(&files, &manifest.flock)?;
    let ssh = required_file(&files, &manifest.openssh)?;
    let flock_yaml = std::str::from_utf8(flock)
        .map_err(|error| TransferError::Bundle(format!("flock YAML is not UTF-8: {error}")))?;
    Config::from_yaml(flock_yaml)?;
    let ssh_text = std::str::from_utf8(ssh)
        .map_err(|error| TransferError::Bundle(format!("SSH config is not UTF-8: {error}")))?;
    validate_imported_ssh(ssh_text, &manifest.ssh.hosts)?;

    let requirements = LocalRequirements {
        environment_variables: manifest.required_environment.clone(),
        ssh_usernames: manifest.ssh.usernames.clone(),
        identity_files: manifest
            .ssh
            .identities
            .iter()
            .map(|identity| identity.label.clone())
            .collect(),
    };
    let mappings = if request.non_interactive {
        ImportMappings::default()
    } else {
        interaction
            .mappings(&requirements)
            .map_err(TransferError::Interaction)?
    };
    validate_mappings(&mappings)?;
    let rendered_ssh = apply_mappings(ssh_text, &manifest.ssh, &mappings)?;

    let user_ssh_config = render_include(
        &request.user_ssh_config_path,
        &request.owned_ssh_config_path,
    )?;
    transactional_write(&[
        (&request.config_path, flock_yaml.as_bytes()),
        (&request.owned_ssh_config_path, rendered_ssh.as_bytes()),
        (&request.user_ssh_config_path, &user_ssh_config),
    ])?;

    // Re-load the actual destination before any runtime work.
    let installed = Config::load(&request.config_path)?;
    let diagnostics = collect_doctor(
        &installed,
        DoctorRequest {
            target: "all".to_owned(),
        },
        doctor,
    )?;
    let mut unresolved = manifest
        .required_environment
        .iter()
        .filter(|name| env::var_os(name).is_none())
        .map(|name| format!("set required environment variable {name}"))
        .collect::<Vec<_>>();
    if !manifest.ssh.identities.is_empty() && mappings.identity_file.is_none() {
        unresolved.push(format!(
            "map an SSH identity key for: {}",
            requirements.identity_files.join(", ")
        ));
    }
    unresolved.sort();

    Ok(ImportReport {
        schema_version: 1,
        bundle_version: manifest.bundle_version,
        config_path: request.config_path.clone(),
        ssh_config_path: request.owned_ssh_config_path.clone(),
        imported_ssh_hosts: manifest.ssh.hosts,
        unresolved,
        validation: "configuration is valid".to_owned(),
        diagnostics,
    })
}

pub fn write_export_json(output: &mut dyn Write, report: &ExportReport) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *output, report)?;
    writeln!(output)
}

pub fn write_export_human(output: &mut dyn Write, report: &ExportReport) -> io::Result<()> {
    writeln!(
        output,
        "exported {} SSH host(s) to {}",
        report.ssh_hosts.len(),
        report.bundle.display()
    )?;
    if !report.required_environment.is_empty() {
        writeln!(
            output,
            "required environment: {}",
            report.required_environment.join(", ")
        )?;
    }
    Ok(())
}

pub fn write_import_json(output: &mut dyn Write, report: &ImportReport) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *output, report)?;
    writeln!(output)
}

pub fn write_import_human(output: &mut dyn Write, report: &ImportReport) -> io::Result<()> {
    writeln!(
        output,
        "imported configuration to {}",
        report.config_path.display()
    )?;
    writeln!(
        output,
        "installed Opilio SSH config at {}",
        report.ssh_config_path.display()
    )?;
    writeln!(output, "static validation: passed")?;
    writeln!(
        output,
        "targeted diagnostics: {:?}",
        report.diagnostics.exit_status()
    )?;
    for item in &report.unresolved {
        writeln!(output, "unresolved: {item}")?;
    }
    Ok(())
}

fn read_string(path: &Path) -> Result<String, TransferError> {
    fs::read_to_string(path).map_err(|source| TransferError::Read {
        path: path.to_owned(),
        source,
    })
}

fn append_archive_file(
    archive: &mut tar::Builder<&mut Vec<u8>>,
    name: &str,
    contents: &[u8],
) -> Result<(), TransferError> {
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o600);
    header.set_cksum();
    archive
        .append_data(&mut header, name, contents)
        .map_err(|error| TransferError::Bundle(error.to_string()))
}

fn read_archive(path: &Path) -> Result<BTreeMap<String, Vec<u8>>, TransferError> {
    let file = fs::File::open(path).map_err(|source| TransferError::Read {
        path: path.to_owned(),
        source,
    })?;
    let mut archive = tar::Archive::new(file);
    let entries = archive
        .entries()
        .map_err(|error| TransferError::Bundle(format!("malformed archive: {error}")))?;
    let mut files = BTreeMap::new();
    for entry in entries {
        let mut entry = entry
            .map_err(|error| TransferError::Bundle(format!("malformed archive entry: {error}")))?;
        let path = entry
            .path()
            .map_err(|error| TransferError::Bundle(format!("invalid bundle path: {error}")))?;
        if !safe_member_path(&path) {
            return Err(TransferError::Bundle(format!(
                "unsafe bundle path `{}`",
                path.display()
            )));
        }
        if !entry.header().entry_type().is_file() {
            return Err(TransferError::Bundle(format!(
                "bundle member `{}` is not a regular file",
                path.display()
            )));
        }
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(TransferError::Bundle(format!(
                "bundle member `{}` exceeds size limit",
                path.display()
            )));
        }
        let name = path.to_string_lossy().into_owned();
        if !matches!(name.as_str(), MANIFEST_PATH | FLOCK_PATH | SSH_PATH) {
            return Err(TransferError::Bundle(format!(
                "unexpected bundle member `{name}`"
            )));
        }
        let mut contents = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut contents)
            .map_err(|error| TransferError::Bundle(format!("cannot read `{name}`: {error}")))?;
        if files.insert(name.clone(), contents).is_some() {
            return Err(TransferError::Bundle(format!(
                "duplicate bundle member `{name}`"
            )));
        }
    }
    Ok(files)
}

fn safe_member_path(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && path.components().count() == 1
}

fn required_file<'a>(
    files: &'a BTreeMap<String, Vec<u8>>,
    name: &str,
) -> Result<&'a [u8], TransferError> {
    files
        .get(name)
        .map(Vec::as_slice)
        .ok_or_else(|| TransferError::Bundle(format!("missing bundle member `{name}`")))
}

fn validate_manifest_paths(manifest: &BundleManifest) -> Result<(), TransferError> {
    if manifest.flock != FLOCK_PATH || manifest.openssh != SSH_PATH {
        return Err(TransferError::Bundle(
            "manifest contains unsupported or unsafe member paths".to_owned(),
        ));
    }
    Ok(())
}

struct SelectedSsh {
    config: String,
    metadata: SshMetadata,
}

fn select_ssh_entries<'a>(
    source: &str,
    roots: impl Iterator<Item = &'a str>,
) -> Result<SelectedSsh, TransferError> {
    let blocks = parse_ssh(source)?;
    let mut wanted = roots.map(str::to_owned).collect::<BTreeSet<_>>();
    let mut queue = wanted.iter().cloned().collect::<VecDeque<_>>();
    while let Some(host) = queue.pop_front() {
        let Some(block) = find_host_block(&blocks, &host) else {
            continue;
        };
        for (key, value) in &block.directives {
            if key.eq_ignore_ascii_case("proxyjump") {
                for jump in proxy_jump_hosts(value)? {
                    if wanted.insert(jump.clone()) {
                        queue.push_back(jump);
                    }
                }
            }
        }
    }

    let mut selected = Vec::new();
    for block in blocks {
        let hosts = block
            .hosts
            .iter()
            .filter(|host| wanted.contains(*host))
            .cloned()
            .collect::<Vec<_>>();
        if !hosts.is_empty() {
            if let Some((key, _)) = block
                .directives
                .iter()
                .find(|(key, _)| dangerous_directive(key))
            {
                return Err(TransferError::SshConfig(format!(
                    "unsafe directive `{key}` in selected Host entry"
                )));
            }
            selected.push(HostBlock {
                hosts,
                directives: block.directives,
            });
        }
    }
    let found = selected
        .iter()
        .flat_map(|block| block.hosts.iter().cloned())
        .collect::<BTreeSet<_>>();
    let mut usernames = BTreeSet::new();
    let mut identities = Vec::new();
    for block in &selected {
        for (key, value) in &block.directives {
            if key.eq_ignore_ascii_case("user") {
                usernames.insert(value.clone());
            } else if key.eq_ignore_ascii_case("identityfile") {
                let label = value
                    .rsplit(['/', '\\'])
                    .next()
                    .filter(|part| !part.is_empty())
                    .unwrap_or("SSH identity")
                    .to_owned();
                identities.push(IdentityRequirement {
                    label,
                    hosts: block.hosts.clone(),
                });
            }
        }
    }
    let config = render_ssh(&selected, None, None, &BTreeSet::new());
    Ok(SelectedSsh {
        config,
        metadata: SshMetadata {
            hosts: found.into_iter().collect(),
            usernames: usernames.into_iter().collect(),
            identities,
        },
    })
}

fn parse_ssh(source: &str) -> Result<Vec<HostBlock>, TransferError> {
    let mut blocks = Vec::<HostBlock>::new();
    let mut current: Option<HostBlock> = None;
    for (index, raw) in source.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once(char::is_whitespace)
            .map(|(key, value)| (key, value.trim()))
            .ok_or_else(|| TransferError::SshConfig(format!("line {} has no value", index + 1)))?;
        if key.eq_ignore_ascii_case("host") {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            let hosts = value
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if hosts.is_empty()
                || hosts
                    .iter()
                    .any(|host| host.starts_with('!') || host.contains(['*', '?']))
            {
                current = None;
            } else {
                current = Some(HostBlock {
                    hosts,
                    directives: Vec::new(),
                });
            }
        } else if key.eq_ignore_ascii_case("match") {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
        } else if let Some(block) = &mut current {
            if !key.eq_ignore_ascii_case("include") {
                block.directives.push((key.to_owned(), value.to_owned()));
            }
        }
    }
    if let Some(block) = current {
        blocks.push(block);
    }
    Ok(blocks)
}

fn dangerous_directive(key: &str) -> bool {
    ["localcommand", "proxycommand", "knownhostscommand"]
        .iter()
        .any(|candidate| key.eq_ignore_ascii_case(candidate))
}

fn find_host_block<'a>(blocks: &'a [HostBlock], host: &str) -> Option<&'a HostBlock> {
    blocks
        .iter()
        .find(|block| block.hosts.iter().any(|candidate| candidate == host))
}

fn proxy_jump_hosts(value: &str) -> Result<Vec<String>, TransferError> {
    if value.eq_ignore_ascii_case("none") {
        return Ok(Vec::new());
    }
    value
        .split(',')
        .map(|hop| {
            let host = hop
                .trim()
                .rsplit_once('@')
                .map_or(hop.trim(), |(_, host)| host)
                .trim_matches(['[', ']'])
                .split(':')
                .next()
                .unwrap_or("");
            if host.is_empty() || host.chars().any(char::is_whitespace) {
                Err(TransferError::SshConfig(format!(
                    "invalid ProxyJump hop `{hop}`"
                )))
            } else {
                Ok(host.to_owned())
            }
        })
        .collect()
}

fn render_ssh(
    blocks: &[HostBlock],
    username: Option<&str>,
    identity_file: Option<&Path>,
    identity_hosts: &BTreeSet<String>,
) -> String {
    let mut output =
        String::from("# Generated by Opilio. Regenerate or remove this file safely.\n");
    for block in blocks {
        output.push_str("\nHost ");
        output.push_str(&block.hosts.join(" "));
        output.push('\n');
        for (key, value) in &block.directives {
            if key.eq_ignore_ascii_case("identityfile") {
                continue;
            }
            output.push_str("  ");
            output.push_str(key);
            output.push(' ');
            if key.eq_ignore_ascii_case("user") {
                output.push_str(username.unwrap_or(value));
            } else {
                output.push_str(value);
            }
            output.push('\n');
        }
        if block.hosts.iter().any(|host| identity_hosts.contains(host))
            && let Some(path) = identity_file
        {
            output.push_str("  IdentityFile ");
            output.push_str(&path.to_string_lossy().replace('\\', "/"));
            output.push('\n');
        }
    }
    output
}

fn validate_imported_ssh(source: &str, expected_hosts: &[String]) -> Result<(), TransferError> {
    if source.contains("IdentityFile") {
        return Err(TransferError::Bundle(
            "portable SSH config must not contain controller-local IdentityFile directives"
                .to_owned(),
        ));
    }
    let blocks = parse_ssh(source)?;
    if let Some((key, _)) = blocks
        .iter()
        .flat_map(|block| &block.directives)
        .find(|(key, _)| dangerous_directive(key))
    {
        return Err(TransferError::Bundle(format!(
            "unsafe SSH directive `{key}` in bundle"
        )));
    }
    let actual = blocks
        .iter()
        .flat_map(|block| block.hosts.iter().cloned())
        .collect::<BTreeSet<_>>();
    let expected = expected_hosts.iter().cloned().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(TransferError::Bundle(
            "SSH host metadata does not match bundled SSH config".to_owned(),
        ));
    }
    Ok(())
}

fn validate_mappings(mappings: &ImportMappings) -> Result<(), TransferError> {
    if mappings
        .username
        .as_deref()
        .is_some_and(|value| value.trim().is_empty() || value.chars().any(char::is_whitespace))
    {
        return Err(TransferError::Interaction(
            "SSH username must be one non-empty token".to_owned(),
        ));
    }
    if mappings
        .identity_file
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(TransferError::Interaction(
            "SSH identity path must not be empty".to_owned(),
        ));
    }
    Ok(())
}

fn apply_mappings(
    source: &str,
    metadata: &SshMetadata,
    mappings: &ImportMappings,
) -> Result<String, TransferError> {
    let blocks = parse_ssh(source)?;
    let identity_hosts = metadata
        .identities
        .iter()
        .flat_map(|identity| identity.hosts.iter().cloned())
        .collect::<BTreeSet<_>>();
    Ok(render_ssh(
        &blocks,
        mappings.username.as_deref(),
        mappings.identity_file.as_deref(),
        &identity_hosts,
    ))
}

fn render_include(user_config: &Path, owned_config: &Path) -> Result<Vec<u8>, TransferError> {
    let parent = user_config.parent().ok_or_else(|| {
        TransferError::Bundle("user SSH config has no parent directory".to_owned())
    })?;
    let include_path = owned_config.strip_prefix(parent).map_err(|_| {
        TransferError::Bundle(
            "Opilio-owned SSH config must be inside the user SSH directory".to_owned(),
        )
    })?;
    if include_path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(TransferError::Bundle(
            "unsafe Opilio SSH include path".to_owned(),
        ));
    }
    let directive = format!(
        "Include {}",
        include_path.to_string_lossy().replace('\\', "/")
    );
    let existing = if user_config.exists() {
        read_string(user_config)?
    } else {
        String::new()
    };
    if existing.lines().any(|line| line.trim() == directive) {
        return Ok(existing.into_bytes());
    }
    let mut updated = format!("{directive}\n");
    updated.push_str(&existing);
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    Ok(updated.into_bytes())
}

fn transactional_write(outputs: &[(&PathBuf, &[u8])]) -> Result<(), TransferError> {
    transactional_write_inner(outputs, None)
}

fn transactional_write_inner(
    outputs: &[(&PathBuf, &[u8])],
    fail_before_commit: Option<usize>,
) -> Result<(), TransferError> {
    struct Staged {
        destination: PathBuf,
        temporary: PathBuf,
        original: Option<Vec<u8>>,
    }

    let mut staged: Vec<Staged> = Vec::with_capacity(outputs.len());
    for (index, (destination, contents)) in outputs.iter().enumerate() {
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|source| TransferError::Write {
            path: parent.to_owned(),
            source,
        })?;
        let name = destination
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                TransferError::Bundle("destination path has no valid file name".to_owned())
            })?;
        let temporary = parent.join(format!(
            ".{name}.opilio-stage-{}-{index}",
            std::process::id()
        ));
        let original = match fs::read(destination) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(source) => {
                for item in &staged {
                    let _ = fs::remove_file(&item.temporary);
                }
                return Err(TransferError::Read {
                    path: (*destination).clone(),
                    source,
                });
            }
        };
        let write_result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(contents)?;
            file.sync_all()
        })();
        if let Err(source) = write_result {
            let _ = fs::remove_file(&temporary);
            for item in &staged {
                let _ = fs::remove_file(&item.temporary);
            }
            return Err(TransferError::Write {
                path: (*destination).clone(),
                source,
            });
        }
        staged.push(Staged {
            destination: (*destination).clone(),
            temporary,
            original,
        });
    }

    let mut committed = 0;
    let mut attempted = 0;
    let result = (|| {
        for (index, item) in staged.iter().enumerate() {
            if fail_before_commit == Some(index) {
                return Err(io::Error::other("injected import commit failure"));
            }
            attempted = index + 1;
            replace_file(&item.temporary, &item.destination)?;
            committed += 1;
        }
        Ok(())
    })();
    if let Err(source) = result {
        for item in staged[..attempted].iter().rev() {
            match &item.original {
                Some(contents) => {
                    let _ = atomic_write(&item.destination, contents);
                }
                None => {
                    let _ = fs::remove_file(&item.destination);
                }
            }
        }
        for item in &staged[committed..] {
            let _ = fs::remove_file(&item.temporary);
        }
        return Err(TransferError::Write {
            path: staged
                .get(committed)
                .map_or_else(PathBuf::new, |item| item.destination.clone()),
            source,
        });
    }
    Ok(())
}

fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(source, destination)
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), TransferError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| TransferError::Write {
        path: parent.to_owned(),
        source,
    })?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            TransferError::Bundle("destination path has no valid file name".to_owned())
        })?;
    let temporary = parent.join(format!(".{name}.opilio-new-{}", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }

    result.map_err(|source| TransferError::Write {
        path: path.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn transaction_restores_every_original_after_second_or_third_commit_failure() {
        for failure in [1, 2] {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/opilio-transfer-unit")
                .join(format!(
                    "{}-{}",
                    std::process::id(),
                    NEXT_CASE.fetch_add(1, Ordering::Relaxed)
                ));
            fs::create_dir_all(&root).unwrap();
            let paths = [
                root.join("config.yaml"),
                root.join("owned-ssh"),
                root.join("user-ssh"),
            ];
            for (index, path) in paths.iter().enumerate() {
                fs::write(path, format!("original-{index}")).unwrap();
            }
            let replacements = [b"new-config".as_slice(), b"new-owned", b"new-user"];

            let error = transactional_write_inner(
                &[
                    (&paths[0], replacements[0]),
                    (&paths[1], replacements[1]),
                    (&paths[2], replacements[2]),
                ],
                Some(failure),
            )
            .unwrap_err();

            assert!(error.to_string().contains("injected import commit failure"));
            for (index, path) in paths.iter().enumerate() {
                assert_eq!(
                    fs::read_to_string(path).unwrap(),
                    format!("original-{index}")
                );
            }
        }
    }
}
