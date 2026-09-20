use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use opilio::ssh::{
    CancellationToken, ExecutionOptions, OpenSsh, ProcessAdapter, ProcessError, ProcessOutput,
    ProcessRequest, RemoteInvocation,
};

#[derive(Default)]
struct FakeProcess {
    requests: Mutex<Vec<ProcessRequest>>,
    outputs: Mutex<Vec<ProcessOutput>>,
}

impl FakeProcess {
    fn with_outputs(outputs: Vec<ProcessOutput>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            outputs: Mutex::new(outputs.into_iter().rev().collect()),
        }
    }

    fn requests(&self) -> Vec<ProcessRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl ProcessAdapter for FakeProcess {
    fn find_executable(&self, name: &str) -> Option<PathBuf> {
        (name == "ssh").then(|| PathBuf::from("/system/bin/ssh"))
    }

    fn run(&self, request: &ProcessRequest) -> Result<ProcessOutput, ProcessError> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(self.outputs.lock().unwrap().pop().unwrap_or_default())
    }

    fn interactive(&self, request: &ProcessRequest) -> Result<i32, ProcessError> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(0)
    }
}

fn success() -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(0),
        stdout: b"ok".to_vec(),
        stderr: Vec::new(),
        timed_out: false,
        cancelled: false,
        stdout_truncated: false,
        stderr_truncated: false,
    }
}

#[test]
fn discovers_system_ssh_and_preserves_the_configured_host_alias() {
    let process = Arc::new(FakeProcess::with_outputs(vec![success()]));
    let ssh = OpenSsh::discover(process.clone()).unwrap();

    let result = ssh
        .execute(
            "spark-from-ssh-config",
            &RemoteInvocation::command("printf '%s' \"$HOME\"", "/bin/sh"),
            ExecutionOptions::default(),
        )
        .unwrap();

    assert!(result.success());
    assert_eq!(result.stdout, b"ok");
    let request = &process.requests()[0];
    assert_eq!(request.program(), Path::new("/system/bin/ssh"));
    assert_eq!(
        request.arguments(),
        [
            OsString::from("spark-from-ssh-config"),
            OsString::from("'/bin/sh' '-lc' 'printf '\"'\"'%s'\"'\"' \"$HOME\"'")
        ]
    );
}

#[test]
fn structured_exec_quotes_every_remote_argument_and_working_directory() {
    let process = Arc::new(FakeProcess::with_outputs(vec![success()]));
    let ssh = OpenSsh::discover(process.clone()).unwrap();
    let invocation = RemoteInvocation::exec(
        "weird program",
        ["plain", "two words", "quote's", "", "$(touch nope)"],
    )
    .with_cwd("~/my app");

    ssh.execute("alpha", &invocation, ExecutionOptions::default())
        .unwrap();

    assert_eq!(
        process.requests()[0].arguments(),
        [
            OsString::from("alpha"),
            OsString::from(
                "cd -- \"$HOME\"/'my app' && exec 'weird program' 'plain' 'two words' 'quote'\"'\"'s' '' '$(touch nope)'"
            )
        ]
    );
}

#[test]
fn execution_forwards_timeout_cancellation_and_capture_bound_to_adapter() {
    let process = Arc::new(FakeProcess::with_outputs(vec![ProcessOutput {
        timed_out: true,
        stdout: b"oversized output".to_vec(),
        stderr: b"oversized error".to_vec(),
        ..success()
    }]));
    let ssh = OpenSsh::discover(process.clone()).unwrap();
    let cancellation = CancellationToken::new();
    let options = ExecutionOptions {
        timeout: Some(Duration::from_secs(7)),
        cancellation: cancellation.clone(),
        capture_limit: 4,
    };

    let result = ssh
        .execute(
            "alpha",
            &RemoteInvocation::command("sleep 99", "/bin/bash"),
            options,
        )
        .unwrap();

    assert!(result.timed_out);
    assert_eq!(result.stdout, b"tput");
    assert_eq!(result.stderr, b"rror");
    assert!(result.stdout_truncated);
    assert!(result.stderr_truncated);
    let request = &process.requests()[0];
    assert_eq!(request.timeout(), Some(Duration::from_secs(7)));
    assert_eq!(request.capture_limit(), 4);
    assert!(!request.cancellation().is_cancelled());
    cancellation.cancel();
    assert!(request.cancellation().is_cancelled());
}

#[test]
fn interactive_handoff_uses_inherited_stdio_and_only_the_host_alias() {
    let process = Arc::new(FakeProcess::default());
    let ssh = OpenSsh::discover(process.clone()).unwrap();

    let exit_code = ssh.interactive("alpha-alias").unwrap();

    assert_eq!(exit_code, 0);
    assert!(process.requests()[0].is_interactive());
    assert_eq!(
        process.requests()[0].arguments(),
        [OsString::from("alpha-alias")]
    );
}

#[test]
fn control_master_starts_reuses_and_closes_the_same_temporary_socket() {
    let process = Arc::new(FakeProcess::with_outputs(vec![
        success(),
        success(),
        success(),
    ]));
    let ssh = OpenSsh::discover(process.clone()).unwrap();
    let control_path = Path::new("target/opilio-tests/control-%C");

    let master = ssh.start_control_master("alpha", control_path).unwrap();
    master
        .execute(
            &RemoteInvocation::exec("uname", ["-a"]),
            ExecutionOptions::default(),
        )
        .unwrap();
    master.close().unwrap();

    let requests = process.requests();
    assert_eq!(
        requests[0].arguments(),
        [
            "-o",
            "ControlMaster=yes",
            "-o",
            "ControlPersist=no",
            "-o",
            "ControlPath=target/opilio-tests/control-%C",
            "-N",
            "-f",
            "alpha",
        ]
        .map(OsString::from)
    );
    assert_eq!(
        requests[1].arguments(),
        [
            OsString::from("-o"),
            OsString::from("ControlPath=target/opilio-tests/control-%C"),
            OsString::from("alpha"),
            OsString::from("exec 'uname' '-a'"),
        ]
    );
    assert_eq!(
        requests[2].arguments(),
        [
            "-o",
            "ControlPath=target/opilio-tests/control-%C",
            "-O",
            "exit",
            "alpha",
        ]
        .map(OsString::from)
    );
}

#[test]
fn failed_or_timed_out_control_master_start_is_not_reusable() {
    let process = Arc::new(FakeProcess::with_outputs(vec![ProcessOutput {
        exit_code: Some(255),
        stderr: b"connection refused".to_vec(),
        ..ProcessOutput::default()
    }]));
    let ssh = OpenSsh::discover(process).unwrap();

    let error = ssh
        .start_control_master("alpha", Path::new("target/control"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("connection refused"));
}
