//! System OpenSSH execution and connection multiplexing.

use std::{
    env,
    ffi::OsString,
    fmt,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const DEFAULT_CAPTURE_LIMIT: usize = 64 * 1024;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Cooperative cancellation shared between a caller and a running process.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Runtime limits for one remote invocation.
#[derive(Clone, Debug)]
pub struct ExecutionOptions {
    pub timeout: Option<Duration>,
    pub cancellation: CancellationToken,
    pub capture_limit: usize,
}

impl Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            timeout: None,
            cancellation: CancellationToken::new(),
            capture_limit: DEFAULT_CAPTURE_LIMIT,
        }
    }
}

/// A remote action expressed either as a shell command or a structured exec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteInvocation {
    Command {
        command: String,
        shell: String,
        cwd: Option<String>,
    },
    Exec {
        program: String,
        arguments: Vec<String>,
        cwd: Option<String>,
    },
}

impl RemoteInvocation {
    pub fn command(command: impl Into<String>, shell: impl Into<String>) -> Self {
        Self::Command {
            command: command.into(),
            shell: shell.into(),
            cwd: None,
        }
    }

    pub fn exec<I, S>(program: impl Into<String>, arguments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::Exec {
            program: program.into(),
            arguments: arguments.into_iter().map(Into::into).collect(),
            cwd: None,
        }
    }

    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        match &mut self {
            Self::Command {
                cwd: invocation_cwd,
                ..
            }
            | Self::Exec {
                cwd: invocation_cwd,
                ..
            } => *invocation_cwd = Some(cwd.into()),
        }
        self
    }

    fn remote_command(&self) -> String {
        match self {
            Self::Command {
                command,
                shell,
                cwd,
            } => {
                let payload = in_working_directory(cwd.as_deref(), command);
                format!("{} '-lc' {}", quote_posix(shell), quote_posix(&payload))
            }
            Self::Exec {
                program,
                arguments,
                cwd,
            } => {
                let mut command = format!("exec {}", quote_posix(program));
                for argument in arguments {
                    command.push(' ');
                    command.push_str(&quote_posix(argument));
                }
                in_working_directory(cwd.as_deref(), &command)
            }
        }
    }
}

fn in_working_directory(cwd: Option<&str>, command: &str) -> String {
    cwd.map_or_else(
        || command.to_owned(),
        |cwd| format!("cd -- {} && {command}", quote_working_directory(cwd)),
    )
}

fn quote_working_directory(cwd: &str) -> String {
    if cwd == "~" {
        return "\"$HOME\"".to_owned();
    }
    cwd.strip_prefix("~/").map_or_else(
        || quote_posix(cwd),
        |relative| format!("\"$HOME\"/{}", quote_posix(relative)),
    )
}

fn quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Complete local process request, suitable for recording in platform fakes.
#[derive(Clone, Debug)]
pub struct ProcessRequest {
    program: PathBuf,
    arguments: Vec<OsString>,
    timeout: Option<Duration>,
    cancellation: CancellationToken,
    capture_limit: usize,
    interactive: bool,
}

impl ProcessRequest {
    fn captured(program: PathBuf, arguments: Vec<OsString>, options: ExecutionOptions) -> Self {
        Self {
            program,
            arguments,
            timeout: options.timeout,
            cancellation: options.cancellation,
            capture_limit: options.capture_limit,
            interactive: false,
        }
    }

    fn interactive(program: PathBuf, arguments: Vec<OsString>) -> Self {
        Self {
            program,
            arguments,
            timeout: None,
            cancellation: CancellationToken::new(),
            capture_limit: 0,
            interactive: true,
        }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }

    pub const fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    pub const fn capture_limit(&self) -> usize {
        self.capture_limit
    }

    pub const fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    pub const fn is_interactive(&self) -> bool {
        self.interactive
    }
}

/// Bounded output and termination state from a process.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessOutput {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

impl ProcessOutput {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out && !self.cancelled
    }
}

/// Test seam around executable discovery and process execution.
pub trait ProcessAdapter: Send + Sync {
    fn find_executable(&self, name: &str) -> Option<PathBuf>;
    fn run(&self, request: &ProcessRequest) -> Result<ProcessOutput, ProcessError>;
    fn interactive(&self, request: &ProcessRequest) -> Result<i32, ProcessError>;
}

/// Host process implementation used by the executable.
#[derive(Debug, Default)]
pub struct SystemProcessAdapter;

impl ProcessAdapter for SystemProcessAdapter {
    fn find_executable(&self, name: &str) -> Option<PathBuf> {
        find_on_path(name)
    }

    fn run(&self, request: &ProcessRequest) -> Result<ProcessOutput, ProcessError> {
        let mut child = Command::new(&request.program)
            .args(&request.arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(ProcessError::Spawn)?;
        let stdout = child
            .stdout
            .take()
            .ok_or(ProcessError::MissingPipe("stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or(ProcessError::MissingPipe("stderr"))?;
        let stdout_reader = spawn_bounded_reader(stdout, request.capture_limit);
        let stderr_reader = spawn_bounded_reader(stderr, request.capture_limit);
        let started = Instant::now();
        let mut timed_out = false;
        let mut cancelled = false;

        let status = loop {
            if let Some(status) = child.try_wait().map_err(ProcessError::Wait)? {
                break status;
            }
            cancelled = request.cancellation.is_cancelled();
            timed_out = request
                .timeout
                .is_some_and(|timeout| started.elapsed() >= timeout);
            if cancelled || timed_out {
                child.kill().map_err(ProcessError::Kill)?;
                break child.wait().map_err(ProcessError::Wait)?;
            }
            thread::sleep(PROCESS_POLL_INTERVAL);
        };
        let (stdout, stdout_truncated) = join_reader(stdout_reader)?;
        let (stderr, stderr_truncated) = join_reader(stderr_reader)?;

        Ok(ProcessOutput {
            exit_code: status.code(),
            stdout,
            stderr,
            timed_out,
            cancelled,
            stdout_truncated,
            stderr_truncated,
        })
    }

    fn interactive(&self, request: &ProcessRequest) -> Result<i32, ProcessError> {
        Command::new(&request.program)
            .args(&request.arguments)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(ProcessError::Spawn)
            .map(|status| status.code().unwrap_or(1))
    }
}

fn spawn_bounded_reader(
    mut reader: impl Read + Send + 'static,
    limit: usize,
) -> thread::JoinHandle<Result<(Vec<u8>, bool), io::Error>> {
    thread::spawn(move || {
        let mut captured = Vec::with_capacity(limit.min(8192));
        let mut buffer = [0_u8; 8192];
        let mut truncated = false;
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                return Ok((captured, truncated));
            }
            captured.extend_from_slice(&buffer[..read]);
            if captured.len() > limit {
                let excess = captured.len() - limit;
                captured.drain(..excess);
                truncated = true;
            }
        }
    })
}

fn join_reader(
    reader: thread::JoinHandle<Result<(Vec<u8>, bool), io::Error>>,
) -> Result<(Vec<u8>, bool), ProcessError> {
    reader
        .join()
        .map_err(|_| ProcessError::ReaderPanicked)?
        .map_err(ProcessError::Read)
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 {
        return is_executable(candidate).then(|| candidate.to_owned());
    }
    let paths = env::var_os("PATH")?;
    let extensions = executable_extensions();
    env::split_paths(&paths)
        .flat_map(|directory| {
            extensions
                .iter()
                .map(move |extension| directory.join(format!("{name}{extension}")))
        })
        .find(|path| is_executable(path))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(windows)]
fn executable_extensions() -> Vec<String> {
    env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned())
        .split(';')
        .map(|extension| extension.to_ascii_lowercase())
        .chain(std::iter::once(String::new()))
        .collect()
}

#[cfg(not(windows))]
fn executable_extensions() -> Vec<String> {
    vec![String::new()]
}

#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("could not start process: {0}")]
    Spawn(io::Error),
    #[error("could not wait for process: {0}")]
    Wait(io::Error),
    #[error("could not terminate process: {0}")]
    Kill(io::Error),
    #[error("could not read process output: {0}")]
    Read(io::Error),
    #[error("process did not expose its {0} pipe")]
    MissingPipe(&'static str),
    #[error("process output reader panicked")]
    ReaderPanicked,
}

#[derive(Clone)]
pub struct OpenSsh {
    process: Arc<dyn ProcessAdapter>,
    executable: PathBuf,
}

impl fmt::Debug for OpenSsh {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenSsh")
            .field("executable", &self.executable)
            .finish_non_exhaustive()
    }
}

impl OpenSsh {
    pub fn discover(process: Arc<dyn ProcessAdapter>) -> Result<Self, SshError> {
        let executable = process
            .find_executable("ssh")
            .ok_or(SshError::Unavailable)?;
        Ok(Self {
            process,
            executable,
        })
    }

    pub fn system() -> Result<Self, SshError> {
        Self::discover(Arc::new(SystemProcessAdapter))
    }

    pub fn execute(
        &self,
        target: &str,
        invocation: &RemoteInvocation,
        options: ExecutionOptions,
    ) -> Result<ProcessOutput, SshError> {
        self.execute_with_prefix(
            Vec::new(),
            target,
            Some(invocation.remote_command()),
            options,
        )
    }

    fn execute_with_prefix(
        &self,
        mut arguments: Vec<OsString>,
        target: &str,
        remote_command: Option<String>,
        options: ExecutionOptions,
    ) -> Result<ProcessOutput, SshError> {
        arguments.push(target.into());
        if let Some(remote_command) = remote_command {
            arguments.push(remote_command.into());
        }
        let capture_limit = options.capture_limit;
        let mut output = self
            .process
            .run(&ProcessRequest::captured(
                self.executable.clone(),
                arguments,
                options,
            ))
            .map_err(SshError::Process)?;
        bound_output(
            &mut output.stdout,
            &mut output.stdout_truncated,
            capture_limit,
        );
        bound_output(
            &mut output.stderr,
            &mut output.stderr_truncated,
            capture_limit,
        );
        Ok(output)
    }

    pub fn interactive(&self, target: &str) -> Result<i32, SshError> {
        self.process
            .interactive(&ProcessRequest::interactive(
                self.executable.clone(),
                vec![target.into()],
            ))
            .map_err(SshError::Process)
    }

    pub fn start_control_master(
        &self,
        target: impl Into<String>,
        control_path: &Path,
    ) -> Result<ControlMaster, SshError> {
        self.start_control_master_with_options(target, control_path, ExecutionOptions::default())
    }

    pub fn start_control_master_with_options(
        &self,
        target: impl Into<String>,
        control_path: &Path,
        options: ExecutionOptions,
    ) -> Result<ControlMaster, SshError> {
        let target = target.into();
        let control_option = control_path_option(control_path);
        let arguments = [
            OsString::from("-o"),
            OsString::from("ControlMaster=yes"),
            OsString::from("-o"),
            OsString::from("ControlPersist=no"),
            OsString::from("-o"),
            control_option.clone(),
            OsString::from("-N"),
            OsString::from("-f"),
        ]
        .into();
        let result = self.execute_with_prefix(arguments, &target, None, options)?;
        ensure_success(&result)?;
        Ok(ControlMaster {
            ssh: self.clone(),
            target,
            control_option,
            closed: AtomicBool::new(false),
        })
    }
}

impl InteractiveSsh for OpenSsh {
    fn interactive(&self, target: &str) -> Result<i32, SshError> {
        OpenSsh::interactive(self, target)
    }
}

pub trait InteractiveSsh {
    fn interactive(&self, target: &str) -> Result<i32, SshError>;
}

pub struct ControlMaster {
    ssh: OpenSsh,
    target: String,
    control_option: OsString,
    closed: AtomicBool,
}

impl fmt::Debug for ControlMaster {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlMaster")
            .field("target", &self.target)
            .field("control_option", &self.control_option)
            .field("closed", &self.closed.load(Ordering::Acquire))
            .finish()
    }
}

impl ControlMaster {
    pub fn execute(
        &self,
        invocation: &RemoteInvocation,
        options: ExecutionOptions,
    ) -> Result<ProcessOutput, SshError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(SshError::ControlMasterClosed);
        }
        self.ssh.execute_with_prefix(
            vec![OsString::from("-o"), self.control_option.clone()],
            &self.target,
            Some(invocation.remote_command()),
            options,
        )
    }

    pub fn close(&self) -> Result<(), SshError> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let result = self
            .ssh
            .execute_with_prefix(
                vec![
                    OsString::from("-o"),
                    self.control_option.clone(),
                    OsString::from("-O"),
                    OsString::from("exit"),
                ],
                &self.target,
                None,
                ExecutionOptions::default(),
            )
            .and_then(|result| ensure_success(&result));
        if result.is_err() {
            self.closed.store(false, Ordering::Release);
        }
        result
    }
}

impl Drop for ControlMaster {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn control_path_option(control_path: &Path) -> OsString {
    let mut option = OsString::from("ControlPath=");
    option.push(control_path.as_os_str());
    option
}

fn bound_output(output: &mut Vec<u8>, truncated: &mut bool, limit: usize) {
    if output.len() > limit {
        output.drain(..output.len() - limit);
        *truncated = true;
    }
}

fn ensure_success(result: &ProcessOutput) -> Result<(), SshError> {
    if result.success() {
        return Ok(());
    }
    Err(SshError::RemoteFailed {
        exit_code: result.exit_code,
        timed_out: result.timed_out,
        cancelled: result.cancelled,
        stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
    })
}

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("system OpenSSH executable `ssh` was not found on PATH")]
    Unavailable,
    #[error(transparent)]
    Process(#[from] ProcessError),
    #[error(
        "OpenSSH failed (exit {exit_code:?}, timed_out={timed_out}, cancelled={cancelled}): {stderr}"
    )]
    RemoteFailed {
        exit_code: Option<i32>,
        timed_out: bool,
        cancelled: bool,
        stderr: String,
    },
    #[error("ControlMaster session is already closed")]
    ControlMasterClosed,
}
