use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    path::PathBuf,
    sync::Arc,
    sync::Mutex,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::Duration,
};

use opilio::{
    config::Config,
    lifecycle::{
        DeviceLifecycleResult, LifecycleError, LifecycleExecutor, LifecycleOperation,
        LifecycleRequest, LifecycleState, PlannedStep, RecoveryEvent, RecoveryStage,
        SystemLifecycleExecutor, execute_lifecycle, execute_recovery_cycle, plan_lifecycle,
        write_human, write_json,
    },
    power::PowerCapabilities,
    ssh::{ProcessAdapter, ProcessError, ProcessOutput, ProcessRequest},
    status::ExitStatus,
};

struct FailingSudoProcess;

impl ProcessAdapter for FailingSudoProcess {
    fn find_executable(&self, name: &str) -> Option<PathBuf> {
        (name == "ssh").then(|| PathBuf::from("/test/ssh"))
    }

    fn run(&self, _request: &ProcessRequest) -> Result<ProcessOutput, ProcessError> {
        Ok(ProcessOutput {
            exit_code: Some(1),
            stderr: b"sudo: a password is required".to_vec(),
            ..ProcessOutput::default()
        })
    }

    fn interactive(&self, _request: &ProcessRequest) -> Result<i32, ProcessError> {
        unreachable!()
    }
}

const CONFIG: &str = r#"
sites:
  home:
    label: Home
devices:
  shelly:
    site: home
    ssh: shelly-host
    groups: [fleet]
    power:
      type: shelly
      host: shelly.local
    shutdown:
      command: sudo shutdown -h now
      cut_power: true
      timeout: 30s
  wol:
    site: home
    ssh: wol-host
    groups: [fleet]
    power:
      type: wol
      mac: "00:11:22:33:44:55"
  bare:
    site: home
    ssh: bare-host
    groups: [fleet]
groups:
  fleet:
    devices: [shelly, wol, bare]
"#;

#[derive(Default)]
struct FakeExecutor {
    calls: Mutex<Vec<(String, PlannedStep)>>,
    failures: Mutex<BTreeMap<(String, PlannedStep), String>>,
    once: Mutex<Vec<(String, PlannedStep, String)>>,
}

impl FakeExecutor {
    fn fail(&self, device: &str, step: PlannedStep, error: &str) {
        self.failures
            .lock()
            .unwrap()
            .insert((device.to_owned(), step), error.to_owned());
    }

    fn calls_for(&self, device: &str) -> Vec<PlannedStep> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == device)
            .map(|(_, step)| *step)
            .collect()
    }

    fn fail_once(&self, device: &str, step: PlannedStep, error: &str) {
        self.once
            .lock()
            .unwrap()
            .push((device.to_owned(), step, error.to_owned()));
    }
}

impl LifecycleExecutor for FakeExecutor {
    fn execute_step(
        &self,
        device_name: &str,
        _device: &opilio::domain::Device,
        step: PlannedStep,
        _timeout: Duration,
    ) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push((device_name.to_owned(), step));
        let mut once = self.once.lock().unwrap();
        if let Some(index) = once
            .iter()
            .position(|(device, candidate, _)| device == device_name && *candidate == step)
        {
            return Err(once.remove(index).2);
        }
        self.failures
            .lock()
            .unwrap()
            .get(&(device_name.to_owned(), step))
            .cloned()
            .map_or(Ok(()), Err)
    }
}

fn request(operation: LifecycleOperation, target: &str) -> LifecycleRequest {
    LifecycleRequest {
        operation,
        target: target.to_owned(),
        confirmed: false,
        force: false,
        wait: false,
        parallelism: NonZeroUsize::MIN,
    }
}

#[test]
fn production_executor_explains_noninteractive_sudo_failures_without_echoing_output() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let ssh = opilio::ssh::OpenSsh::discover(Arc::new(FailingSudoProcess)).unwrap();
    let executor = SystemLifecycleExecutor::new(ssh);

    let error = executor
        .execute_step(
            "shelly",
            &config.devices()["shelly"],
            PlannedStep::GracefulShutdown,
            Duration::from_secs(5),
        )
        .unwrap_err();

    assert!(error.contains("non-interactive sudo authorization"));
    assert!(!error.contains("a password is required"));
}

#[test]
fn yes_confirms_but_cannot_turn_normal_off_into_a_forced_cut() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let mut request = request(LifecycleOperation::Off, "shelly");
    request.confirmed = true;

    let plan = plan_lifecycle(&config, request).unwrap();

    assert_eq!(
        plan.devices[0].steps,
        vec![
            PlannedStep::GracefulShutdown,
            PlannedStep::WaitForShutdown,
            PlannedStep::CutPhysicalPower,
        ]
    );
}

#[test]
fn physical_power_off_and_cycle_require_force_even_when_confirmed() {
    let config = Config::from_yaml(CONFIG).unwrap();
    for operation in [LifecycleOperation::PowerOff, LifecycleOperation::PowerCycle] {
        let mut request = request(operation, "shelly");
        request.confirmed = true;
        assert!(matches!(
            plan_lifecycle(&config, request),
            Err(LifecycleError::ForceRequired(_))
        ));
    }
}

#[test]
fn force_selects_only_explicit_bypass_cut_and_cycle_plans() {
    let config = Config::from_yaml(CONFIG).unwrap();
    for (operation, expected) in [
        (LifecycleOperation::Off, vec![PlannedStep::CutPhysicalPower]),
        (
            LifecycleOperation::PowerOff,
            vec![PlannedStep::CutPhysicalPower],
        ),
        (
            LifecycleOperation::PowerCycle,
            vec![PlannedStep::CutPhysicalPower, PlannedStep::RequestPowerOn],
        ),
    ] {
        let mut request = request(operation, "shelly");
        request.force = true;
        request.confirmed = true;
        let plan = plan_lifecycle(&config, request).unwrap();
        assert_eq!(plan.devices[0].steps, expected);
    }
}

#[test]
fn tui_recovery_cycle_waits_off_before_restoring_power() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let executor = FakeExecutor::default();
    let mut events = Vec::new();
    let mut held = Vec::new();
    let outcome = execute_recovery_cycle(
        "shelly",
        &config.devices()["shelly"],
        &executor,
        |duration| held.push(duration),
        |event| events.push(event),
    )
    .unwrap();

    assert_eq!(outcome.graceful_error, None);
    assert_eq!(held, [Duration::from_secs(10)]);
    assert_eq!(
        executor.calls_for("shelly"),
        [
            PlannedStep::GracefulShutdown,
            PlannedStep::WaitForShutdown,
            PlannedStep::CutPhysicalPower,
            PlannedStep::RequestPowerOn,
        ]
    );
    assert!(events.contains(&RecoveryEvent::Started(RecoveryStage::HoldPower)));
    assert!(events.contains(&RecoveryEvent::Succeeded(RecoveryStage::HoldPower)));
}

#[test]
fn tui_recovery_cycle_forces_cut_after_failed_shutdown_and_stops_if_cut_fails() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let executor = FakeExecutor::default();
    executor.fail("shelly", PlannedStep::GracefulShutdown, "SSH timed out");
    let mut events = Vec::new();
    let outcome = execute_recovery_cycle(
        "shelly",
        &config.devices()["shelly"],
        &executor,
        |_| {},
        |event| events.push(event),
    )
    .unwrap();
    assert_eq!(
        outcome.graceful_error.as_deref(),
        Some("graceful shutdown: SSH timed out")
    );
    assert_eq!(
        executor.calls_for("shelly"),
        [
            PlannedStep::GracefulShutdown,
            PlannedStep::CutPhysicalPower,
            PlannedStep::RequestPowerOn,
        ]
    );
    assert!(events.contains(&RecoveryEvent::Failed(
        RecoveryStage::GracefulShutdown,
        "SSH timed out".into()
    )));

    let executor = FakeExecutor::default();
    executor.fail(
        "shelly",
        PlannedStep::CutPhysicalPower,
        "Shelly unavailable",
    );
    let error = execute_recovery_cycle(
        "shelly",
        &config.devices()["shelly"],
        &executor,
        |_| panic!("must not pause when cut failed"),
        |_| {},
    )
    .unwrap_err();
    assert!(error.contains("Shelly unavailable"));
    assert!(
        !executor
            .calls_for("shelly")
            .contains(&PlannedStep::RequestPowerOn)
    );
}

#[test]
fn tui_recovery_cycle_falls_back_after_shutdown_wait_and_reports_restore_failure() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let executor = FakeExecutor::default();
    executor.fail("shelly", PlannedStep::WaitForShutdown, "still reachable");
    executor.fail("shelly", PlannedStep::RequestPowerOn, "HTTP 429");
    let mut events = Vec::new();
    let error = execute_recovery_cycle(
        "shelly",
        &config.devices()["shelly"],
        &executor,
        |_| {},
        |event| events.push(event),
    )
    .unwrap_err();

    assert!(error.contains("outlet may remain off"), "{error}");
    assert!(error.contains("HTTP 429"), "{error}");
    assert!(events.contains(&RecoveryEvent::Failed(
        RecoveryStage::WaitForShutdown,
        "still reachable".into()
    )));
    assert!(
        executor
            .calls_for("shelly")
            .contains(&PlannedStep::CutPhysicalPower)
    );
}

#[test]
fn tui_recovery_cycle_retries_rate_limited_power_on_once() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let executor = FakeExecutor::default();
    executor.fail_once("shelly", PlannedStep::RequestPowerOn, "HTTP 429");
    let mut waits = Vec::new();
    let mut events = Vec::new();
    execute_recovery_cycle(
        "shelly",
        &config.devices()["shelly"],
        &executor,
        |duration| waits.push(duration),
        |event| events.push(event),
    )
    .unwrap();

    assert_eq!(waits, [Duration::from_secs(10), Duration::from_secs(10)]);
    assert_eq!(
        executor
            .calls_for("shelly")
            .iter()
            .filter(|step| **step == PlannedStep::RequestPowerOn)
            .count(),
        2
    );
    assert!(events.contains(&RecoveryEvent::Failed(
        RecoveryStage::RestorePower,
        "HTTP 429".into()
    )));
    assert!(events.contains(&RecoveryEvent::Started(RecoveryStage::WaitForShelly)));
    assert!(events.contains(&RecoveryEvent::Succeeded(RecoveryStage::RetryPowerOn)));
}

#[test]
fn tui_recovery_cycle_rejects_non_shelly_provider_before_ssh() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let executor = FakeExecutor::default();
    for device in ["wol", "bare"] {
        let result = execute_recovery_cycle(
            device,
            &config.devices()[device],
            &executor,
            |_| panic!("must not pause without physical power provider"),
            |_| panic!("must not attempt a step without physical power provider"),
        );
        assert!(result.unwrap_err().contains("physical power provider"));
        assert!(executor.calls_for(device).is_empty());
    }
}

#[test]
fn missing_shelly_secret_blocks_cycle_before_graceful_shutdown() {
    let config = Config::from_yaml(
        "devices:\n  spark:\n    ssh: spark\n    power:\n      type: shelly\n      host: 127.0.0.1\n      auth:\n        password: \"${env:OPILIO_TEST_MISSING_RECOVERY_SECRET_0029}\"\n",
    )
    .unwrap();
    let executor = SystemLifecycleExecutor::system();

    let error = executor
        .prepare_power("spark", &config.devices()["spark"])
        .unwrap_err();
    assert!(error.contains("OPILIO_TEST_MISSING_RECOVERY_SECRET_0029"));
}

#[test]
fn collection_confirmation_names_every_resolved_device() {
    let config = Config::from_yaml(CONFIG).unwrap();

    let plan = plan_lifecycle(&config, request(LifecycleOperation::Reboot, "home")).unwrap();

    assert_eq!(
        plan.confirmation_device_names(),
        Some(vec![
            "bare".to_owned(),
            "shelly".to_owned(),
            "wol".to_owned()
        ])
    );
}

#[test]
fn even_a_one_device_group_requires_confirmation_and_force_always_confirms() {
    let config = Config::from_yaml(
        &CONFIG.replace(
            "groups:\n  fleet:\n    devices: [shelly, wol, bare]",
            "groups:\n  fleet:\n    devices: [shelly, wol, bare]\n  one:\n    devices: [shelly]",
        )
        .replacen(
            "groups: [fleet]\n    power:",
            "groups: [fleet, one]\n    power:",
            1,
        ),
    )
    .unwrap();
    let group = plan_lifecycle(&config, request(LifecycleOperation::Shutdown, "one")).unwrap();
    assert_eq!(
        group.confirmation_device_names(),
        Some(vec!["shelly".to_owned()])
    );

    let mut forced = request(LifecycleOperation::PowerOff, "shelly");
    forced.force = true;
    let forced = plan_lifecycle(&config, forced).unwrap();
    assert_eq!(
        forced.confirmation_device_names(),
        Some(vec!["shelly".to_owned()])
    );
}

#[test]
fn wol_only_off_degrades_to_graceful_shutdown_without_claiming_a_cut() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let plan = plan_lifecycle(&config, request(LifecycleOperation::Off, "wol")).unwrap();
    let executor = FakeExecutor::default();

    let report = execute_lifecycle(&config, plan, &executor).unwrap();

    assert_eq!(
        executor.calls_for("wol"),
        vec![PlannedStep::GracefulShutdown, PlannedStep::WaitForShutdown]
    );
    assert_eq!(report.devices[0].state, LifecycleState::Unreachable);
    assert!(!report.devices[0].physical_power_cut);
}

#[test]
fn normal_off_timeout_never_falls_through_to_physical_cut() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let plan = plan_lifecycle(&config, request(LifecycleOperation::Off, "shelly")).unwrap();
    let executor = FakeExecutor::default();
    executor.fail("shelly", PlannedStep::WaitForShutdown, "shutdown timed out");

    let report = execute_lifecycle(&config, plan, &executor).unwrap();

    assert_eq!(
        executor.calls_for("shelly"),
        vec![PlannedStep::GracefulShutdown, PlannedStep::WaitForShutdown]
    );
    assert_eq!(report.devices[0].state, LifecycleState::Unknown);
    assert_eq!(
        report.devices[0].error.as_deref(),
        Some("shutdown timed out")
    );
}

#[test]
fn on_wait_stops_at_ssh_readiness() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let mut request = request(LifecycleOperation::On, "wol");
    request.wait = true;
    let plan = plan_lifecycle(&config, request).unwrap();
    let executor = FakeExecutor::default();

    let report = execute_lifecycle(&config, plan, &executor).unwrap();

    assert_eq!(
        executor.calls_for("wol"),
        vec![PlannedStep::RequestPowerOn, PlannedStep::WaitForSsh]
    );
    assert_eq!(report.devices[0].state, LifecycleState::SshReady);
}

#[test]
fn provider_capabilities_control_legal_plans() {
    let config = Config::from_yaml(CONFIG).unwrap();

    let on_bare = plan_lifecycle(&config, request(LifecycleOperation::On, "bare"));
    assert!(matches!(on_bare, Err(LifecycleError::Unsupported { .. })));

    let mut forced_wol = request(LifecycleOperation::PowerOff, "wol");
    forced_wol.force = true;
    assert!(matches!(
        plan_lifecycle(&config, forced_wol),
        Err(LifecycleError::Unsupported { .. })
    ));

    assert_eq!(
        opilio::lifecycle::configured_capabilities(&config.devices()["shelly"]),
        PowerCapabilities {
            can_request_power_on: true,
            can_cut_physical_power: true,
        }
    );
}

#[test]
fn unsupported_devices_do_not_prevent_supported_collection_members() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let mut request = request(LifecycleOperation::On, "fleet");
    request.confirmed = true;
    let plan = plan_lifecycle(&config, request).unwrap();

    let report = execute_lifecycle(&config, plan, &FakeExecutor::default()).unwrap();

    assert_eq!(report.summary.succeeded, 2);
    assert_eq!(report.summary.failed, 1);
    assert_eq!(report.devices[0].device, "bare");
    assert!(
        report.devices[0]
            .error
            .as_deref()
            .unwrap()
            .contains("no power-on")
    );
}

#[test]
fn failures_continue_per_device_and_aggregate_partial_exit() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let mut request = request(LifecycleOperation::Shutdown, "fleet");
    request.confirmed = true;
    let plan = plan_lifecycle(&config, request).unwrap();
    let executor = FakeExecutor::default();
    executor.fail("wol", PlannedStep::GracefulShutdown, "SSH unavailable");

    let report = execute_lifecycle(&config, plan, &executor).unwrap();

    assert_eq!(report.summary.succeeded, 2);
    assert_eq!(report.summary.failed, 1);
    assert_eq!(report.exit_status(), ExitStatus::PartialSuccess);
    assert_eq!(executor.calls.lock().unwrap().len(), 3);
}

struct BoundedExecutor {
    active: AtomicUsize,
    maximum: AtomicUsize,
}

impl LifecycleExecutor for BoundedExecutor {
    fn execute_step(
        &self,
        _device_name: &str,
        _device: &opilio::domain::Device,
        _step: PlannedStep,
        _timeout: Duration,
    ) -> Result<(), String> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(active, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(10));
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn lifecycle_is_sequential_by_default_and_honors_a_bounded_override() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let executor = BoundedExecutor {
        active: AtomicUsize::new(0),
        maximum: AtomicUsize::new(0),
    };
    let mut sequential = request(LifecycleOperation::Shutdown, "fleet");
    sequential.confirmed = true;
    execute_lifecycle(
        &config,
        plan_lifecycle(&config, sequential).unwrap(),
        &executor,
    )
    .unwrap();
    assert_eq!(executor.maximum.load(Ordering::SeqCst), 1);

    executor.maximum.store(0, Ordering::SeqCst);
    let mut parallel = request(LifecycleOperation::Shutdown, "fleet");
    parallel.confirmed = true;
    parallel.parallelism = NonZeroUsize::new(2).unwrap();
    execute_lifecycle(
        &config,
        plan_lifecycle(&config, parallel).unwrap(),
        &executor,
    )
    .unwrap();
    assert_eq!(executor.maximum.load(Ordering::SeqCst), 2);
}

#[test]
fn human_and_json_results_preserve_truthful_device_states() {
    let config = Config::from_yaml(CONFIG).unwrap();
    let plan = plan_lifecycle(&config, request(LifecycleOperation::On, "shelly")).unwrap();
    let report = execute_lifecycle(&config, plan, &FakeExecutor::default()).unwrap();
    let mut human = Vec::new();
    let mut json = Vec::new();

    write_human(&mut human, &report).unwrap();
    write_json(&mut json, &report).unwrap();

    assert_eq!(
        String::from_utf8(human).unwrap(),
        "shelly: booting\n1 succeeded, 0 failed\n"
    );
    let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["operation"], "on");
    assert_eq!(value["devices"][0]["state"], "booting");
    assert_eq!(value["devices"][0]["physical_power_cut"], false);
}

#[allow(dead_code)]
fn assert_result_is_public(_: &DeviceLifecycleResult) {}
