//! Persistent, rotating operation history.

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

use chrono::{SecondsFormat, Utc};
use clap::ValueEnum;
use directories::ProjectDirs;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DEFAULT_FAILURE_TAIL_BYTES: usize = 64 * 1024;
pub const DEFAULT_MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
pub const DEFAULT_MAX_FILES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryConfig {
    pub directory: PathBuf,
    pub max_file_bytes: u64,
    pub max_files: usize,
    pub failure_tail_bytes: usize,
}

impl HistoryConfig {
    pub fn platform_default() -> Result<Self, HistoryError> {
        let directory = env::var_os("OPILIO_HISTORY_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                ProjectDirs::from("", "", "opilio")
                    .map(|directories| directories.data_local_dir().join("history"))
            })
            .ok_or(HistoryError::DataDirectoryUnavailable)?;
        Ok(Self {
            directory,
            max_file_bytes: environment_number(
                "OPILIO_HISTORY_MAX_FILE_BYTES",
                DEFAULT_MAX_FILE_BYTES,
            )?,
            max_files: environment_number("OPILIO_HISTORY_MAX_FILES", DEFAULT_MAX_FILES)?,
            failure_tail_bytes: environment_number(
                "OPILIO_HISTORY_FAILURE_TAIL_BYTES",
                DEFAULT_FAILURE_TAIL_BYTES,
            )?,
        })
    }
}

fn environment_number<T>(name: &str, default: T) -> Result<T, HistoryError>
where
    T: std::str::FromStr,
{
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|_| HistoryError::InvalidConfig(format!("{name} must be a positive integer"))),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(HistoryError::InvalidConfig(format!(
            "could not read {name}: {error}"
        ))),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum OperationSource {
    Cli,
    Tui,
    Scheduled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryResult {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewHistoryRecord {
    pub source: OperationSource,
    pub operation: String,
    pub action: Option<String>,
    pub requested_target: String,
    pub resolved_device: String,
    pub duration_ms: u64,
    pub result: HistoryResult,
    pub exit_code: Option<i32>,
    pub force: bool,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRecord {
    pub schema_version: u8,
    pub id: String,
    pub timestamp: String,
    pub source: OperationSource,
    pub operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    pub requested_target: String,
    pub resolved_device: String,
    pub duration_ms: u64,
    pub result: HistoryResult,
    pub exit_code: Option<i32>,
    pub force: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryWarning {
    pub file: String,
    pub line: usize,
    pub kind: HistoryWarningKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryWarningKind {
    MalformedRecord,
    TruncatedRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryListing {
    pub schema_version: u8,
    pub records: Vec<OperationRecord>,
    pub warnings: Vec<HistoryWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryDetail {
    pub schema_version: u8,
    pub record: OperationRecord,
    pub warnings: Vec<HistoryWarning>,
}

#[derive(Debug, Clone, Default)]
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    pub fn new(values: impl IntoIterator<Item = String>) -> Self {
        let mut secrets = values
            .into_iter()
            .filter(|value| !value.is_empty())
            .flat_map(|value| {
                let encoded =
                    url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>();
                let encoded_lowercase = encoded.to_ascii_lowercase();
                [value, encoded, encoded_lowercase]
            })
            .collect::<Vec<_>>();
        secrets.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
        secrets.dedup();
        Self { secrets }
    }

    pub fn redact(&self, value: &str) -> String {
        self.secrets
            .iter()
            .fold(value.to_owned(), |redacted, secret| {
                redacted.replace(secret, "[REDACTED]")
            })
    }
}

#[derive(Debug, Clone)]
pub struct HistoryStore {
    config: HistoryConfig,
    redactor: Redactor,
}

impl HistoryStore {
    pub fn new(config: HistoryConfig) -> Result<Self, HistoryError> {
        Self::with_redactor(config, Redactor::default())
    }

    pub fn with_redactor(config: HistoryConfig, redactor: Redactor) -> Result<Self, HistoryError> {
        if config.max_files == 0 {
            return Err(HistoryError::InvalidConfig(
                "max_files must be at least one".to_owned(),
            ));
        }
        if config.max_file_bytes == 0 {
            return Err(HistoryError::InvalidConfig(
                "max_file_bytes must be at least one".to_owned(),
            ));
        }
        fs::create_dir_all(&config.directory)?;
        Ok(Self { config, redactor })
    }

    pub fn platform_default(redactor: Redactor) -> Result<Self, HistoryError> {
        Self::with_redactor(HistoryConfig::platform_default()?, redactor)
    }

    pub fn append(&self, input: NewHistoryRecord) -> Result<OperationRecord, HistoryError> {
        let record = self.prepare(input);
        let mut encoded = serde_json::to_vec(&record)?;
        encoded.push(b'\n');
        let lock = self.open_lock()?;
        FileExt::lock_exclusive(&lock)?;
        let result = (|| {
            let needs_separator = self.active_needs_separator()?;
            let rotated =
                self.rotate_if_needed(encoded.len() as u64 + u64::from(needs_separator))?;
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.active_path())?;
            if needs_separator && !rotated {
                file.write_all(b"\n")?;
            }
            file.write_all(&encoded)?;
            file.sync_data()?;
            Ok(record)
        })();
        fs2::FileExt::unlock(&lock)?;
        result
    }

    pub fn list(&self, target: Option<&str>) -> Result<HistoryListing, HistoryError> {
        self.list_matching(|record| {
            target.is_none_or(|target| {
                record.requested_target == target || record.resolved_device == target
            })
        })
    }

    pub fn list_for_target(
        &self,
        requested_target: &str,
        resolved_devices: &[String],
    ) -> Result<HistoryListing, HistoryError> {
        self.list_matching(|record| {
            record.requested_target == requested_target
                || resolved_devices.contains(&record.resolved_device)
        })
    }

    fn list_matching(
        &self,
        predicate: impl Fn(&OperationRecord) -> bool,
    ) -> Result<HistoryListing, HistoryError> {
        let lock = self.open_lock()?;
        FileExt::lock_shared(&lock)?;
        let result = self.read_records(predicate);
        fs2::FileExt::unlock(&lock)?;
        result
    }

    pub fn show(&self, id: &str) -> Result<(OperationRecord, Vec<HistoryWarning>), HistoryError> {
        let listing = self.list(None)?;
        listing
            .records
            .into_iter()
            .find(|record| record.id == id)
            .map(|record| (record, listing.warnings))
            .ok_or_else(|| HistoryError::NotFound(id.to_owned()))
    }

    fn prepare(&self, input: NewHistoryRecord) -> OperationRecord {
        let failure = input.result == HistoryResult::Failed;
        OperationRecord {
            schema_version: 1,
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            source: input.source,
            operation: self.redactor.redact(&input.operation),
            action: input.action.map(|value| self.redactor.redact(&value)),
            requested_target: self.redactor.redact(&input.requested_target),
            resolved_device: self.redactor.redact(&input.resolved_device),
            duration_ms: input.duration_ms,
            result: input.result,
            exit_code: input.exit_code,
            force: input.force,
            stdout: failure.then(|| {
                bounded_tail(
                    &self
                        .redactor
                        .redact(input.stdout.as_deref().unwrap_or_default()),
                    self.config.failure_tail_bytes,
                )
            }),
            stderr: failure.then(|| {
                bounded_tail(
                    &self
                        .redactor
                        .redact(input.stderr.as_deref().unwrap_or_default()),
                    self.config.failure_tail_bytes,
                )
            }),
            error: failure.then_some(input.error).flatten().map(|value| {
                bounded_tail(
                    &self.redactor.redact(&value),
                    self.config.failure_tail_bytes,
                )
            }),
        }
    }

    fn read_records(
        &self,
        predicate: impl Fn(&OperationRecord) -> bool,
    ) -> Result<HistoryListing, HistoryError> {
        let mut records = Vec::new();
        let mut warnings = Vec::new();
        for path in self.history_paths_oldest_first() {
            if !path.exists() {
                continue;
            }
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("history")
                .to_owned();
            let mut reader = BufReader::new(File::open(&path)?);
            let mut bytes = Vec::new();
            let mut line_number = 0;
            loop {
                bytes.clear();
                let count = reader.read_until(b'\n', &mut bytes)?;
                if count == 0 {
                    break;
                }
                line_number += 1;
                if !bytes.ends_with(b"\n") {
                    warnings.push(HistoryWarning {
                        file: file_name.clone(),
                        line: line_number,
                        kind: HistoryWarningKind::TruncatedRecord,
                        message: "ignored truncated final JSONL record".to_owned(),
                    });
                    continue;
                }
                match serde_json::from_slice::<OperationRecord>(&bytes) {
                    Ok(record) if predicate(&record) => {
                        records.push(record);
                    }
                    Ok(_) => {}
                    Err(error) => warnings.push(HistoryWarning {
                        file: file_name.clone(),
                        line: line_number,
                        kind: HistoryWarningKind::MalformedRecord,
                        message: error.to_string(),
                    }),
                }
            }
        }
        records.reverse();
        Ok(HistoryListing {
            schema_version: 1,
            records,
            warnings,
        })
    }

    fn active_needs_separator(&self) -> Result<bool, io::Error> {
        let path = self.active_path();
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        if file.metadata()?.len() == 0 {
            return Ok(false);
        }
        file.seek(SeekFrom::End(-1))?;
        let mut byte = [0];
        file.read_exact(&mut byte)?;
        Ok(byte[0] != b'\n')
    }

    fn rotate_if_needed(&self, incoming_bytes: u64) -> Result<bool, io::Error> {
        let active = self.active_path();
        let current_bytes = fs::metadata(&active).map_or(0, |metadata| metadata.len());
        if current_bytes == 0
            || current_bytes.saturating_add(incoming_bytes) <= self.config.max_file_bytes
        {
            return Ok(false);
        }
        let oldest = self.rotated_path(self.config.max_files - 1);
        if self.config.max_files > 1 && oldest.exists() {
            fs::remove_file(oldest)?;
        }
        for index in (1..self.config.max_files.saturating_sub(1)).rev() {
            let from = self.rotated_path(index);
            if from.exists() {
                fs::rename(from, self.rotated_path(index + 1))?;
            }
        }
        if self.config.max_files > 1 {
            fs::rename(active, self.rotated_path(1))?;
        } else {
            fs::remove_file(active)?;
        }
        Ok(true)
    }

    fn history_paths_oldest_first(&self) -> Vec<PathBuf> {
        let mut paths = (1..self.config.max_files)
            .rev()
            .map(|index| self.rotated_path(index))
            .collect::<Vec<_>>();
        paths.push(self.active_path());
        paths
    }

    fn open_lock(&self) -> Result<File, io::Error> {
        OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(self.config.directory.join("history.lock"))
    }

    fn active_path(&self) -> PathBuf {
        self.config.directory.join("history.jsonl")
    }

    fn rotated_path(&self, index: usize) -> PathBuf {
        self.config.directory.join(format!("history.{index}.jsonl"))
    }
}

pub fn write_human(output: &mut dyn Write, listing: &HistoryListing) -> io::Result<()> {
    for record in &listing.records {
        writeln!(
            output,
            "{} {} {} {} {} {}",
            record.id,
            record.timestamp,
            source_name(record.source),
            record.operation,
            record.resolved_device,
            result_name(record.result)
        )?;
    }
    write_warnings(output, &listing.warnings)
}

pub fn write_detail_human(output: &mut dyn Write, detail: &HistoryDetail) -> io::Result<()> {
    let record = &detail.record;
    writeln!(output, "ID: {}", record.id)?;
    writeln!(output, "Timestamp: {}", record.timestamp)?;
    writeln!(output, "Source: {}", source_name(record.source))?;
    writeln!(output, "Operation: {}", record.operation)?;
    if let Some(action) = &record.action {
        writeln!(output, "Action: {action}")?;
    }
    writeln!(output, "Requested target: {}", record.requested_target)?;
    writeln!(output, "Device: {}", record.resolved_device)?;
    writeln!(output, "Duration: {} ms", record.duration_ms)?;
    writeln!(output, "Result: {}", result_name(record.result))?;
    writeln!(
        output,
        "Exit code: {}",
        record
            .exit_code
            .map_or_else(|| "-".to_owned(), |code| code.to_string())
    )?;
    writeln!(output, "Force: {}", record.force)?;
    if let Some(stdout) = &record.stdout {
        writeln!(output, "Stdout:\n{stdout}")?;
    }
    if let Some(stderr) = &record.stderr {
        writeln!(output, "Stderr:\n{stderr}")?;
    }
    if let Some(error) = &record.error {
        writeln!(output, "Error: {error}")?;
    }
    write_warnings(output, &detail.warnings)
}

pub fn write_json<T: Serialize>(output: &mut dyn Write, value: &T) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *output, value).map_err(io::Error::other)?;
    writeln!(output)
}

fn write_warnings(output: &mut dyn Write, warnings: &[HistoryWarning]) -> io::Result<()> {
    for warning in warnings {
        writeln!(
            output,
            "warning: {}:{}: {}",
            warning.file, warning.line, warning.message
        )?;
    }
    Ok(())
}

const fn source_name(source: OperationSource) -> &'static str {
    match source {
        OperationSource::Cli => "cli",
        OperationSource::Tui => "tui",
        OperationSource::Scheduled => "scheduled",
    }
}

const fn result_name(result: HistoryResult) -> &'static str {
    match result {
        HistoryResult::Succeeded => "succeeded",
        HistoryResult::Failed => "failed",
    }
}

fn bounded_tail(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut start = value.len() - max_bytes;
    while !value.is_char_boundary(start) {
        start += 1;
    }
    value[start..].to_owned()
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("could not determine the platform data directory")]
    DataDirectoryUnavailable,
    #[error("invalid history configuration: {0}")]
    InvalidConfig(String),
    #[error("history record `{0}` was not found")]
    NotFound(String),
    #[error("history I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("history JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
