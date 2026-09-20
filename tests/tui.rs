use std::time::{Duration, Instant};

use opilio::{
    config::Config,
    service::ServiceState,
    tui::{
        Dashboard, DashboardSample, DeviceState, Effect, Event, Key, MetricSample, Operation,
        Overlay, PollKind, PollPolicy, render_to_string,
    },
};

fn config() -> Config {
    Config::from_yaml(
        r#"
sites:
  home: { label: Home }
  lab: { label: Lab }
devices:
  alpha:
    site: home
    ssh: alpha
    groups: [gpu]
    telemetry: { provider: nvidia }
    services: [llm]
  beta:
    site: lab
    ssh: beta
    groups: [gpu]
groups:
  gpu:
    devices: [alpha, beta]
actions:
  update:
    command: sudo apt update
services:
  llm:
    health: { url: "http://localhost:8000/health" }
"#,
    )
    .unwrap()
}

#[test]
fn reducer_navigates_filters_and_never_starts_work_inline() {
    let mut dashboard = Dashboard::from_config(&config());
    assert_eq!(dashboard.selected_device(), Some("alpha"));

    assert!(dashboard.update(Event::Key(Key::Down)).is_empty());
    assert_eq!(dashboard.selected_device(), Some("beta"));
    dashboard.update(Event::Key(Key::Search));
    dashboard.update(Event::Key(Key::Char('a')));
    dashboard.update(Event::Key(Key::Enter));
    assert_eq!(dashboard.filter(), "a");
    assert_eq!(dashboard.visible_devices(), vec!["alpha", "beta"]);

    let effects = dashboard.update(Event::Key(Key::Refresh));
    assert_eq!(effects, vec![Effect::PollNow]);
    assert!(!dashboard.busy());
}

#[test]
fn collection_operations_require_confirmation_but_single_device_does_not() {
    let mut dashboard = Dashboard::from_config(&config());
    let effects = dashboard.update(Event::Key(Key::Off));
    assert_eq!(
        effects,
        vec![Effect::Operate {
            operation: Operation::Off,
            target: "alpha".into(),
        }]
    );

    dashboard.update(Event::Key(Key::Left));
    dashboard.update(Event::Key(Key::Down));
    assert_eq!(dashboard.selected_scope_name(), "home");
    assert!(dashboard.update(Event::Key(Key::Reboot)).is_empty());
    assert!(matches!(
        dashboard.overlay(),
        Overlay::Confirm {
            operation: Operation::Reboot,
            ..
        }
    ));
    assert_eq!(
        dashboard.update(Event::Key(Key::Char('y'))),
        vec![Effect::Operate {
            operation: Operation::Reboot,
            target: "home".into(),
        }]
    );
    assert_eq!(dashboard.overlay(), &Overlay::None);
}

#[test]
fn actions_and_help_are_keyboard_driven() {
    let mut dashboard = Dashboard::from_config(&config());
    dashboard.update(Event::Key(Key::Action));
    assert_eq!(dashboard.overlay(), &Overlay::Actions);
    let effects = dashboard.update(Event::Key(Key::Enter));
    assert_eq!(
        effects,
        vec![Effect::RunAction {
            name: "update".into(),
            target: "alpha".into(),
        }]
    );
    dashboard.update(Event::Key(Key::Help));
    assert_eq!(dashboard.overlay(), &Overlay::Help);
    dashboard.update(Event::Key(Key::Escape));
    assert_eq!(dashboard.overlay(), &Overlay::None);
}

#[test]
fn preexecution_failure_clears_and_marks_every_selected_device() {
    let mut dashboard = Dashboard::from_config(&config());
    dashboard.update(Event::Key(Key::Left));
    dashboard.update(Event::Key(Key::Action));
    assert_eq!(
        dashboard.update(Event::Key(Key::Enter)),
        vec![Effect::RunAction {
            name: "update".into(),
            target: "all".into(),
        }]
    );
    assert!(dashboard.busy());

    for device in ["alpha", "beta"] {
        dashboard.update(Event::OperationFinished {
            device: device.to_owned(),
            operation: "action update".to_owned(),
            result: Err("OpenSSH unavailable".to_owned()),
        });
    }

    assert!(!dashboard.busy());
    for device in ["alpha", "beta"] {
        assert_eq!(
            dashboard.device(device).unwrap().recent_failure.as_deref(),
            Some("OpenSSH unavailable")
        );
    }
}

#[test]
fn telemetry_samples_are_bounded_and_keep_service_and_failure_details() {
    let mut dashboard = Dashboard::from_config(&config());
    for value in 0..80 {
        dashboard.update(Event::PollCompleted(DashboardSample {
            device: "alpha".into(),
            state: DeviceState::Running,
            ram_percent: Some(value as f64),
            gpu_percent: Some((value / 2) as f64),
            watts: Some(100.0 + value as f64),
            service: Some(("llm".into(), ServiceState::Loading, Some("Qwen".into()))),
            error: (value == 79).then(|| "probe failed".into()),
        }));
    }
    let device = dashboard.device("alpha").unwrap();
    assert_eq!(device.ram_history.len(), 60);
    assert_eq!(device.ram_history.front(), Some(&MetricSample(20.0)));
    assert_eq!(device.model.as_deref(), Some("Qwen"));
    assert!(
        device
            .recent_failure
            .as_deref()
            .unwrap()
            .contains("probe failed")
    );
}

#[test]
fn adaptive_polling_backs_off_site_loss_and_recovers() {
    let start = Instant::now();
    let mut policy = PollPolicy::new(start);
    assert_eq!(
        policy.due(start),
        vec![PollKind::Telemetry, PollKind::Service]
    );
    policy.mark_polled(PollKind::Telemetry, start);
    policy.mark_polled(PollKind::Service, start);
    assert!(policy.due(start + Duration::from_secs(1)).is_empty());
    assert_eq!(
        policy.due(start + Duration::from_secs(2)),
        vec![PollKind::Telemetry]
    );
    assert_eq!(
        policy.due(start + Duration::from_secs(7)),
        vec![PollKind::Telemetry, PollKind::Service]
    );

    policy.record_site_result("lab", false, start);
    assert_eq!(policy.site_delay("lab"), Duration::from_secs(10));
    policy.record_site_result("lab", false, start + Duration::from_secs(10));
    assert_eq!(policy.site_delay("lab"), Duration::from_secs(20));
    policy.record_site_result("lab", false, start + Duration::from_secs(30));
    assert_eq!(policy.site_delay("lab"), Duration::from_secs(30));

    policy.record_site_result("lab", true, start + Duration::from_secs(60));
    assert_eq!(policy.site_delay("lab"), Duration::from_secs(2));
    assert!(policy.site_due("lab", start + Duration::from_secs(62)));
    policy.manual_refresh(start + Duration::from_secs(61));
    assert!(policy.site_due("lab", start + Duration::from_secs(61)));
}

#[test]
fn stable_renderer_shows_master_detail_status_and_shortcuts() {
    let mut dashboard = Dashboard::from_config(&config());
    dashboard.update(Event::PollCompleted(DashboardSample {
        device: "alpha".into(),
        state: DeviceState::Running,
        ram_percent: Some(72.0),
        gpu_percent: Some(87.0),
        watts: Some(118.0),
        service: Some(("llm".into(), ServiceState::Ready, Some("Qwen".into()))),
        error: None,
    }));
    let rendered = render_to_string(&dashboard, 100, 24);
    for expected in [
        "Sites / Groups",
        "Devices",
        "Details",
        "alpha",
        "running",
        "RAM",
        "GPU",
        "118 W",
        "Qwen",
        "[o] On",
        "[?] Help",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?}\n{rendered}"
        );
    }
}

#[test]
fn every_runtime_state_has_a_distinct_label() {
    let labels = [
        DeviceState::Off,
        DeviceState::Unreachable,
        DeviceState::Running,
        DeviceState::Booting,
        DeviceState::Error,
        DeviceState::Unknown,
    ]
    .map(DeviceState::label);
    assert_eq!(
        labels,
        [
            "off",
            "unreachable",
            "running",
            "booting",
            "error",
            "unknown"
        ]
    );
}
