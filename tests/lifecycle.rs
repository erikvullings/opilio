use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::Mutex,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::Duration,
};

use opilio::{
    config::Config,
    lifecycle::{
        DeviceLifecycleResult, LifecycleError, LifecycleExecutor, LifecycleOperation,
        LifecycleRequest, LifecycleState, PlannedStep, execute_lifecycle, plan_lifecycle,
        write_human, write_json,
    },
    power::PowerCapabilities,
    status::ExitStatus,
};

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
