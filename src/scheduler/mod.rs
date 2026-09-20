//! Owned native scheduled jobs and cross-platform artifact generation.

use std::{
    env, fmt, fs, io,
    path::PathBuf,
    process::{Command, Stdio},
    str::FromStr,
};

use clap::Parser;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

pub mod linux;
pub mod macos;
pub mod windows;

pub const OWNERSHIP_MARKER: &str = "OPILIO_SCHEDULE_V1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Systemd,
    Cron,
    Launchd,
    TaskScheduler,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtTime {
    hour: u8,
    minute: u8,
}

impl AtTime {
    pub const fn hour(self) -> u8 {
        self.hour
    }

    pub const fn minute(self) -> u8 {
        self.minute
    }
}

impl FromStr for AtTime {
    type Err = ScheduleError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (hour, minute) = value
            .split_once(':')
            .ok_or_else(|| ScheduleError::InvalidTime(value.to_owned()))?;
        if hour.len() != 2 || minute.len() != 2 {
            return Err(ScheduleError::InvalidTime(value.to_owned()));
        }
        let hour = hour
            .parse::<u8>()
            .map_err(|_| ScheduleError::InvalidTime(value.to_owned()))?;
        let minute = minute
            .parse::<u8>()
            .map_err(|_| ScheduleError::InvalidTime(value.to_owned()))?;
        if hour > 23 || minute > 59 {
            return Err(ScheduleError::InvalidTime(value.to_owned()));
        }
        Ok(Self { hour, minute })
    }
}

impl fmt::Display for AtTime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:02}:{:02}", self.hour, self.minute)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScheduleId(String);

impl ScheduleId {
    pub fn new(value: impl Into<String>) -> Result<Self, ScheduleError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 63
            && value.bytes().enumerate().all(|(index, byte)| match byte {
                b'a'..=b'z' | b'0'..=b'9' => true,
                b'-' => index > 0 && index + 1 < value.len(),
                _ => false,
            });
        if !valid {
            return Err(ScheduleError::InvalidId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScheduleId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleCommand(Vec<String>);

impl ScheduleCommand {
    pub fn new(args: Vec<String>) -> Result<Self, ScheduleError> {
        if args.is_empty()
            || args.iter().any(|argument| {
                argument.contains(['\0', '\r', '\n'])
                    || matches!(argument.as_str(), "--source" | "--config")
            })
        {
            return Err(ScheduleError::InvalidCommand(args));
        }
        let mut argv = vec!["opilio".to_owned()];
        argv.extend(args.iter().cloned());
        let valid = crate::cli::Cli::try_parse_from(argv)
            .ok()
            .and_then(|cli| cli.command)
            .is_some_and(|command| {
                matches!(
                    command,
                    crate::cli::Command::Status { .. }
                        | crate::cli::Command::On { .. }
                        | crate::cli::Command::Off { .. }
                        | crate::cli::Command::Shutdown { .. }
                        | crate::cli::Command::Reboot { .. }
                        | crate::cli::Command::PowerOff { .. }
                        | crate::cli::Command::PowerCycle { .. }
                        | crate::cli::Command::Action {
                            command: crate::cli::ActionCommand::Run { .. }
                        }
                        | crate::cli::Command::Alias {
                            command: crate::cli::AliasCommand::Run { .. }
                        }
                )
            });
        if !valid {
            return Err(ScheduleError::InvalidCommand(args));
        }
        Ok(Self(args))
    }

    pub fn args(&self) -> &[String] {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct AddSchedule {
    pub id: ScheduleId,
    pub at: AtTime,
    pub command: ScheduleCommand,
    pub executable: PathBuf,
    pub config: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleEntry {
    pub id: String,
    pub at: String,
    pub command: Vec<String>,
    pub backend: Backend,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScheduleListing {
    pub schema_version: u8,
    pub jobs: Vec<ScheduleEntry>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScheduleMutation {
    pub schema_version: u8,
    pub job: ScheduleEntry,
}

pub trait Scheduler {
    fn list(&self) -> Result<ScheduleListing, ScheduleError>;
    fn add(&self, request: AddSchedule) -> Result<ScheduleEntry, ScheduleError>;
    fn remove(&self, id: &ScheduleId) -> Result<ScheduleEntry, ScheduleError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub path: PathBuf,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformPlan {
    pub backend: Backend,
    pub artifacts: Vec<Artifact>,
    pub install: Vec<NativeCommand>,
    pub remove: Vec<NativeCommand>,
}

pub mod platform {
    pub use super::linux::{cron_line, systemd};
    pub use super::macos::launchd;
    pub use super::windows::task_scheduler as windows;
}

#[derive(Debug, Serialize, Deserialize)]
struct OwnedRecord {
    owner: String,
    job: ScheduleEntry,
    artifacts: Vec<PathBuf>,
}

pub struct NativeScheduler {
    backend: Backend,
    registry: PathBuf,
    native_root: PathBuf,
    uid: Option<u32>,
}

impl NativeScheduler {
    pub fn platform_default() -> Result<Self, ScheduleError> {
        let registry = ProjectDirs::from("", "", "opilio")
            .map(|dirs| dirs.data_local_dir().join("schedules"))
            .ok_or(ScheduleError::DataDirectoryUnavailable)?;
        fs::create_dir_all(&registry)?;
        #[cfg(target_os = "linux")]
        {
            let base =
                directories::BaseDirs::new().ok_or(ScheduleError::DataDirectoryUnavailable)?;
            let systemd = command_succeeds("systemctl", &["--user", "show-environment"]);
            return Ok(Self {
                backend: if systemd {
                    Backend::Systemd
                } else {
                    Backend::Cron
                },
                registry,
                native_root: base.home_dir().join(".config/systemd/user"),
                uid: None,
            });
        }
        #[cfg(target_os = "macos")]
        {
            let base =
                directories::BaseDirs::new().ok_or(ScheduleError::DataDirectoryUnavailable)?;
            let uid = Command::new("id")
                .arg("-u")
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .and_then(|value| value.trim().parse().ok())
                .ok_or_else(|| ScheduleError::Native("could not determine user id".into()))?;
            return Ok(Self {
                backend: Backend::Launchd,
                registry,
                native_root: base.home_dir().join("Library/LaunchAgents"),
                uid: Some(uid),
            });
        }
        #[cfg(target_os = "windows")]
        {
            return Ok(Self {
                backend: Backend::TaskScheduler,
                native_root: registry.clone(),
                registry,
                uid: None,
            });
        }
        #[allow(unreachable_code)]
        Err(ScheduleError::UnsupportedPlatform(
            env::consts::OS.to_owned(),
        ))
    }

    fn record_path(&self, id: &ScheduleId) -> PathBuf {
        self.registry.join(format!("{id}.json"))
    }

    fn read_owned(&self, id: &ScheduleId) -> Result<OwnedRecord, ScheduleError> {
        let path = self.record_path(id);
        let bytes = fs::read(&path).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                ScheduleError::NotOwned(id.to_string())
            } else {
                ScheduleError::Io(error)
            }
        })?;
        let record: OwnedRecord = serde_json::from_slice(&bytes)?;
        if record.owner != OWNERSHIP_MARKER || record.job.id != id.as_str() {
            return Err(ScheduleError::NotOwned(id.to_string()));
        }
        Ok(record)
    }

    fn plan(&self, request: &AddSchedule, backend: Backend) -> PlatformPlan {
        match backend {
            Backend::Systemd => linux::systemd(
                &self.native_root,
                &request.id,
                request.at,
                &request.executable,
                &request.config,
                &request.command,
            ),
            Backend::Launchd => macos::launchd(
                &self.native_root,
                self.uid.expect("launchd backend records uid"),
                &request.id,
                request.at,
                &request.executable,
                &request.config,
                &request.command,
            ),
            Backend::TaskScheduler => windows::task_scheduler(
                &self.native_root,
                &request.id,
                request.at,
                &request.executable,
                &request.config,
                &request.command,
            ),
            Backend::Cron => PlatformPlan {
                backend: Backend::Cron,
                artifacts: Vec::new(),
                install: Vec::new(),
                remove: Vec::new(),
            },
        }
    }
}

impl Scheduler for NativeScheduler {
    fn list(&self) -> Result<ScheduleListing, ScheduleError> {
        let mut jobs = Vec::new();
        let mut warnings = Vec::new();
        for entry in fs::read_dir(&self.registry)? {
            let entry = entry?;
            if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            match fs::read(entry.path())
                .map_err(ScheduleError::from)
                .and_then(|bytes| serde_json::from_slice::<OwnedRecord>(&bytes).map_err(Into::into))
            {
                Ok(record) if record.owner == OWNERSHIP_MARKER => {
                    let id = &record.job.id;
                    if record.job.backend == Backend::Cron {
                        match current_crontab() {
                            Ok(contents)
                                if !contents.lines().any(|line| {
                                    line.ends_with(&format!("# {OWNERSHIP_MARKER} id={id}"))
                                }) =>
                            {
                                warnings.push(format!(
                                    "owned schedule {id} is missing from the native scheduler"
                                ));
                            }
                            Err(error) => warnings
                                .push(format!("could not inspect native schedule {id}: {error}")),
                            Ok(_) => {}
                        }
                    } else if record.artifacts.iter().any(|artifact| {
                        fs::read_to_string(artifact)
                            .map(|contents| !contents.contains(OWNERSHIP_MARKER))
                            .unwrap_or(true)
                    }) {
                        warnings.push(format!(
                            "owned schedule {id} has missing or changed native artifacts"
                        ));
                    }
                    jobs.push(record.job);
                }
                Ok(_) => {}
                Err(_) => warnings.push(format!(
                    "ignored malformed Opilio metadata {}",
                    entry.file_name().to_string_lossy()
                )),
            }
        }
        jobs.sort_by(|left, right| left.id.cmp(&right.id));
        warnings.sort();
        Ok(ScheduleListing {
            schema_version: 1,
            jobs,
            warnings,
        })
    }

    fn add(&self, request: AddSchedule) -> Result<ScheduleEntry, ScheduleError> {
        validate_path("Opilio executable", &request.executable)?;
        validate_path("configuration", &request.config)?;
        if self.record_path(&request.id).exists() {
            return Err(ScheduleError::AlreadyExists(request.id.to_string()));
        }
        fs::create_dir_all(&self.registry)?;
        let plan = self.plan(&request, self.backend);
        if self.backend == Backend::Cron {
            install_cron(&request)?;
        } else {
            for artifact in &plan.artifacts {
                if artifact.path.exists() {
                    return Err(ScheduleError::AlreadyExists(request.id.to_string()));
                }
                if let Some(parent) = artifact.path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&artifact.path, &artifact.contents)?;
            }
            if let Err(error) = run_commands(&plan.install) {
                for artifact in &plan.artifacts {
                    let _ = fs::remove_file(&artifact.path);
                }
                if self.backend == Backend::Systemd {
                    let _ = run_commands(&[NativeCommand {
                        program: "systemctl".into(),
                        args: vec!["--user".into(), "daemon-reload".into()],
                    }]);
                }
                return Err(error);
            }
        }
        let job = ScheduleEntry {
            id: request.id.to_string(),
            at: request.at.to_string(),
            command: request.command.args().to_vec(),
            backend: self.backend,
        };
        let record = OwnedRecord {
            owner: OWNERSHIP_MARKER.to_owned(),
            job: job.clone(),
            artifacts: plan
                .artifacts
                .iter()
                .map(|artifact| artifact.path.clone())
                .collect(),
        };
        fs::write(
            self.record_path(&request.id),
            serde_json::to_vec_pretty(&record)?,
        )?;
        Ok(job)
    }

    fn remove(&self, id: &ScheduleId) -> Result<ScheduleEntry, ScheduleError> {
        let record = self.read_owned(id)?;
        if !backend_supported(record.job.backend) {
            return Err(ScheduleError::Native(format!(
                "refusing to remove {id}: recorded backend is invalid on this platform"
            )));
        }
        if record.job.backend == Backend::Cron {
            remove_cron(id)?;
        } else {
            let artifacts = record
                .artifacts
                .iter()
                .map(|artifact| {
                    fs::read_to_string(artifact)
                        .map(|contents| (artifact.clone(), contents))
                        .map_err(|error| {
                            ScheduleError::Native(format!(
                                "refusing to remove {id}: cannot verify {}: {error}",
                                artifact.display()
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            for (artifact, contents) in &artifacts {
                if !contents.contains(OWNERSHIP_MARKER) {
                    return Err(ScheduleError::NotOwned(id.to_string()));
                }
                if !artifact.starts_with(&self.native_root) {
                    return Err(ScheduleError::Native(format!(
                        "refusing to remove {id}: artifact is outside {}: {}",
                        self.native_root.display(),
                        artifact.display()
                    )));
                }
            }
            let request = AddSchedule {
                id: id.clone(),
                at: record.job.at.parse().map_err(|_| {
                    ScheduleError::Native(format!(
                        "refusing to remove {id}: owned metadata has an invalid time"
                    ))
                })?,
                command: ScheduleCommand::new(record.job.command.clone()).map_err(|_| {
                    ScheduleError::Native(format!(
                        "refusing to remove {id}: owned metadata has an invalid command"
                    ))
                })?,
                executable: PathBuf::new(),
                config: PathBuf::new(),
            };
            let plan = self.plan(&request, record.job.backend);
            if let Some(first) = plan.remove.first() {
                run_commands(std::slice::from_ref(first))?;
            }
            for (artifact, _) in &artifacts {
                if let Err(error) = fs::remove_file(artifact) {
                    restore_artifacts(&artifacts);
                    return Err(error.into());
                }
            }
            if plan.remove.len() > 1 {
                if let Err(error) = run_commands(&plan.remove[1..]) {
                    restore_artifacts(&artifacts);
                    return Err(error);
                }
            }
        }
        fs::remove_file(self.record_path(id))?;
        Ok(record.job)
    }
}

fn current_crontab() -> Result<String, ScheduleError> {
    let output = Command::new("crontab").arg("-l").output()?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|_| ScheduleError::Native("crontab output was not UTF-8".into()))
    } else if output.status.code() == Some(1)
        && String::from_utf8_lossy(&output.stderr)
            .to_ascii_lowercase()
            .contains("no crontab")
    {
        Ok(String::new())
    } else {
        Err(native_failure("crontab -l", &output.stderr))
    }
}

fn install_cron(request: &AddSchedule) -> Result<(), ScheduleError> {
    let mut crontab = current_crontab()?;
    let marker = format!("# {OWNERSHIP_MARKER} id={}", request.id);
    if crontab.lines().any(|line| line.ends_with(&marker)) {
        return Err(ScheduleError::AlreadyExists(request.id.to_string()));
    }
    if !crontab.is_empty() && !crontab.ends_with('\n') {
        crontab.push('\n');
    }
    crontab.push_str(&linux::cron_line(
        &request.id,
        request.at,
        &request.executable,
        &request.config,
        &request.command,
    ));
    crontab.push('\n');
    write_crontab(&crontab)
}

fn remove_cron(id: &ScheduleId) -> Result<(), ScheduleError> {
    let marker = format!("# {OWNERSHIP_MARKER} id={id}");
    let current = current_crontab()?;
    if !current.lines().any(|line| line.ends_with(&marker)) {
        return Err(ScheduleError::NotOwned(id.to_string()));
    }
    let retained = current
        .lines()
        .filter(|line| !line.ends_with(&marker))
        .collect::<Vec<_>>()
        .join("\n");
    write_crontab(if retained.is_empty() {
        ""
    } else {
        // The child receives EOF, so a missing final newline is accepted by crontab.
        &retained
    })
}

fn write_crontab(contents: &str) -> Result<(), ScheduleError> {
    let mut child = Command::new("crontab")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    use std::io::Write;
    child
        .stdin
        .take()
        .ok_or_else(|| ScheduleError::Native("could not open crontab stdin".into()))?
        .write_all(contents.as_bytes())?;
    let output = child.wait_with_output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(native_failure("crontab -", &output.stderr))
    }
}

fn run_commands(commands: &[NativeCommand]) -> Result<(), ScheduleError> {
    for command in commands {
        let output = Command::new(&command.program)
            .args(&command.args)
            .output()?;
        if !output.status.success() {
            return Err(native_failure(
                &format!("{} {}", command.program.display(), command.args.join(" ")),
                &output.stderr,
            ));
        }
    }
    Ok(())
}

fn restore_artifacts(artifacts: &[(PathBuf, String)]) {
    for (path, contents) in artifacts {
        let _ = fs::write(path, contents);
    }
}

const fn backend_supported(backend: Backend) -> bool {
    match backend {
        Backend::Systemd | Backend::Cron => cfg!(target_os = "linux"),
        Backend::Launchd => cfg!(target_os = "macos"),
        Backend::TaskScheduler => cfg!(target_os = "windows"),
    }
}

#[cfg(target_os = "linux")]
fn command_succeeds(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn native_failure(command: &str, stderr: &[u8]) -> ScheduleError {
    ScheduleError::Native(format!(
        "`{command}` failed: {}",
        String::from_utf8_lossy(stderr).trim()
    ))
}

fn validate_path(label: &str, path: &std::path::Path) -> Result<(), ScheduleError> {
    let value = path
        .to_str()
        .ok_or_else(|| ScheduleError::InvalidPath(label.to_owned()))?;
    if value.is_empty() || value.contains(['\0', '\r', '\n']) {
        return Err(ScheduleError::InvalidPath(label.to_owned()));
    }
    Ok(())
}

pub fn write_listing_human(
    output: &mut dyn io::Write,
    listing: &ScheduleListing,
) -> io::Result<()> {
    for job in &listing.jobs {
        writeln!(
            output,
            "{}\t{}\t{}\t{}",
            job.id,
            job.at,
            backend_name(job.backend),
            job.command.join(" ")
        )?;
    }
    for warning in &listing.warnings {
        writeln!(output, "warning: {warning}")?;
    }
    Ok(())
}

pub fn write_json(output: &mut dyn io::Write, value: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *output, value)?;
    writeln!(output)
}

fn backend_name(backend: Backend) -> &'static str {
    match backend {
        Backend::Systemd => "systemd",
        Backend::Cron => "cron",
        Backend::Launchd => "launchd",
        Backend::TaskScheduler => "task_scheduler",
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("invalid schedule id `{0}`; use 1-63 lowercase letters, digits, and interior hyphens")]
    InvalidId(String),
    #[error("invalid daily time `{0}`; expected HH:MM")]
    InvalidTime(String),
    #[error("scheduled command is not a supported deterministic Opilio operation: {0:?}")]
    InvalidCommand(Vec<String>),
    #[error("{0} path cannot be represented safely by the native scheduler")]
    InvalidPath(String),
    #[error("schedule `{0}` already exists")]
    AlreadyExists(String),
    #[error("schedule `{0}` is not owned by Opilio")]
    NotOwned(String),
    #[error("native scheduler failed: {0}")]
    Native(String),
    #[error("native scheduling is unsupported on {0}")]
    UnsupportedPlatform(String),
    #[error("could not determine the local scheduler data directory")]
    DataDirectoryUnavailable,
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn registry_ignores_foreign_metadata_and_refuses_its_removal() {
        let registry = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/opilio-scheduler-unit")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT_CASE.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&registry).unwrap();
        fs::write(
            registry.join("foreign.json"),
            r#"{"owner":"someone-else","job":{"id":"foreign","at":"01:00","command":["status","all"],"backend":"launchd"},"artifacts":[]}"#,
        )
        .unwrap();
        let scheduler = NativeScheduler {
            backend: Backend::Launchd,
            native_root: registry.clone(),
            registry,
            uid: Some(501),
        };

        assert!(scheduler.list().unwrap().jobs.is_empty());
        assert!(matches!(
            scheduler.remove(&ScheduleId::new("foreign").unwrap()),
            Err(ScheduleError::NotOwned(id)) if id == "foreign"
        ));
    }
}
