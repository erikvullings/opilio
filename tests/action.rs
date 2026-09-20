use std::{
    num::NonZeroUsize,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use opilio::{
    action::{ActionExecutor, ActionRequest, ActionState, run_action},
    config::Config,
    ssh::{ExecutionOptions, ProcessOutput, RemoteInvocation},
    status::ExitStatus,
};

const CONFIG: &str = r#"
devices:
  alpha:
    ssh: alpha-host
    shell: /bin/bash
    groups: [workers]
  beta:
    ssh: beta-host
    groups: [workers]
  gamma:
    ssh: gamma-host
groups:
  workers:
    devices: [alpha, beta]
actions:
  deploy:
    cwd: /srv/default
    exec:
      program: ./deploy
      args: [--safe]
    timeout: 30s
    overrides:
      workers:
        command: systemctl restart app
        timeout: 10s
      alpha:
        cwd: ~/alpha
        exec:
          program: ./alpha-deploy
          args: ["two words"]
"#;

#[derive(Default)]
struct FakeExecutor {
    calls: Mutex<Vec<(String, RemoteInvocation, Option<Duration>)>>,
    active: AtomicUsize,
    maximum: AtomicUsize,
}

impl ActionExecutor for FakeExecutor {
    fn execute(
        &self,
        ssh_target: &str,
        invocation: &RemoteInvocation,
        options: ExecutionOptions,
    ) -> Result<ProcessOutput, String> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(active, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(10));
        self.calls.lock().unwrap().push((
            ssh_target.to_owned(),
            invocation.clone(),
            options.timeout,
        ));
        self.active.fetch_sub(1, Ordering::SeqCst);
        if ssh_target == "beta-host" {
            Ok(ProcessOutput {
                exit_code: Some(17),
                stderr: b"restart failed".to_vec(),
                ..ProcessOutput::default()
            })
        } else {
            Ok(ProcessOutput {
                exit_code: Some(0),
                stdout: b"done".to_vec(),
                ..ProcessOutput::default()
            })
        }
    }
}

#[test]
fn action_resolution_executes_device_group_and_default_implementations() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let executor = FakeExecutor::default();

    let report = run_action(
        &config,
        ActionRequest {
            name: "deploy".to_owned(),
            target: "all".to_owned(),
            parallelism: NonZeroUsize::new(2).unwrap(),
        },
        &executor,
    )
    .unwrap();

    assert_eq!(report.summary.total, 3);
    assert_eq!(report.summary.succeeded, 2);
    assert_eq!(report.summary.failed, 1);
    assert_eq!(report.exit_status(), ExitStatus::PartialSuccess);
    assert_eq!(report.devices[1].device, "beta");
    assert_eq!(report.devices[1].status, ActionState::Failed);
    assert_eq!(report.devices[1].exit_code, Some(17));
    let calls = executor.calls.lock().unwrap();
    assert!(calls.contains(&(
        "alpha-host".to_owned(),
        RemoteInvocation::Exec {
            program: "./alpha-deploy".to_owned(),
            arguments: vec!["two words".to_owned()],
            cwd: Some("~/alpha".to_owned()),
        },
        Some(Duration::from_secs(30)),
    )));
    assert!(calls.contains(&(
        "beta-host".to_owned(),
        RemoteInvocation::Command {
            command: "systemctl restart app".to_owned(),
            shell: "/bin/sh".to_owned(),
            cwd: Some("/srv/default".to_owned()),
        },
        Some(Duration::from_secs(10)),
    )));
    assert!(calls.contains(&(
        "gamma-host".to_owned(),
        RemoteInvocation::Exec {
            program: "./deploy".to_owned(),
            arguments: vec!["--safe".to_owned()],
            cwd: Some("/srv/default".to_owned()),
        },
        Some(Duration::from_secs(30)),
    )));
}

#[test]
fn disruptive_actions_are_sequential_by_default_and_can_be_bounded() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let sequential = FakeExecutor::default();

    run_action(&config, ActionRequest::new("deploy", "all"), &sequential).unwrap();

    assert_eq!(sequential.maximum.load(Ordering::SeqCst), 1);

    let parallel = FakeExecutor::default();
    run_action(
        &config,
        ActionRequest {
            name: "deploy".to_owned(),
            target: "all".to_owned(),
            parallelism: NonZeroUsize::new(2).unwrap(),
        },
        &parallel,
    )
    .unwrap();
    assert_eq!(parallel.maximum.load(Ordering::SeqCst), 2);
}
