use std::time::{Duration, Instant};

use opilio::{
    config::Config,
    service::ServiceState,
    tui::{
        Dashboard, DashboardSample, DashboardService, DeviceState, Effect, Event, Key,
        MetricSample, Operation, Overlay, PollKind, PollPolicy, render_to_string,
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
            ram_used_bytes: Some(64 * 1024_u64.pow(3)),
            ram_total_bytes: Some(128 * 1024_u64.pow(3)),
            gpu_percent: Some((value / 2) as f64),
            watts: Some(100.0 + value as f64),
            services: Some(vec![
                DashboardService {
                    name: "sglang-main".into(),
                    state: ServiceState::Ready,
                    models: vec!["Qwen3-32B".into()],
                },
                DashboardService {
                    name: "sglang-small".into(),
                    state: ServiceState::Loading,
                    models: vec!["Qwen3-0.6B".into()],
                },
            ]),
            error: (value == 79).then(|| "probe failed".into()),
        }));
    }
    let device = dashboard.device("alpha").unwrap();
    assert_eq!(device.ram_history.len(), 60);
    assert_eq!(device.ram_history.front(), Some(&MetricSample(20.0)));
    assert_eq!(device.services.len(), 2);
    assert_eq!(device.services[0].models, ["Qwen3-32B"]);
    assert!(render_to_string(&dashboard, 120, 30).contains("2m"));
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
        ram_percent: Some(72.4),
        ram_used_bytes: Some(92 * 1024_u64.pow(3)),
        ram_total_bytes: Some(128 * 1024_u64.pow(3)),
        gpu_percent: Some(87.6),
        watts: None,
        services: Some(vec![
            DashboardService {
                name: "sglang-main".into(),
                state: ServiceState::Ready,
                models: vec!["Qwen3-32B".into()],
            },
            DashboardService {
                name: "litellm".into(),
                state: ServiceState::Ready,
                models: vec!["Qwen3-32B".into(), "Qwen3-0.6B".into()],
            },
        ]),
        error: None,
    }));
    let rendered = render_to_string(&dashboard, 120, 30);
    for expected in [
        "Sites / Groups",
        "Devices",
        "Details",
        "alpha",
        "running",
        "RAM used",
        "92.0 / 128.0 GiB",
        "GPU busy",
        "now 72.4%",
        "min 72.4",
        "2s",
        "sglang-main",
        "Qwen3-32B",
        "litellm",
        "ready",
        "Qwen3-0.6B",
        "[o] On",
        "[?] Help",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?}\n{rendered}"
        );
    }
    assert!(!rendered.contains("Power draw"));
    assert!(rendered.contains("Device      alpha  running"));
    assert!(rendered.contains("SSH         alpha"));
    assert!(rendered.contains("RAM used    92.0 / 128.0 GiB (72.4%)"));

    let lines = rendered.lines().collect::<Vec<_>>();
    let last_model = lines
        .iter()
        .position(|line| line.contains("Qwen3-0.6B"))
        .unwrap();
    assert!(!lines[last_model + 1].contains("RAM used"));

    let ram_graph_end = lines
        .iter()
        .position(|line| line.contains("└") && line.contains("─") && line.contains("┘"))
        .unwrap();
    assert!(!lines[ram_graph_end + 1].contains("GPU busy"));
}

#[test]
fn renderer_remains_safe_at_a_compact_terminal_size() {
    let dashboard = Dashboard::from_config(&config());
    let rendered = render_to_string(&dashboard, 80, 20);

    assert!(rendered.contains("Details"));
    assert!(rendered.contains("RAM used"));
}

#[test]
fn power_details_and_history_appear_when_meter_data_exists() {
    let mut dashboard = Dashboard::from_config(&config());
    dashboard.update(Event::PollCompleted(DashboardSample {
        device: "alpha".into(),
        state: DeviceState::Running,
        ram_percent: None,
        ram_used_bytes: None,
        ram_total_bytes: None,
        gpu_percent: None,
        watts: Some(118.4),
        services: None,
        error: None,
    }));

    let rendered = render_to_string(&dashboard, 120, 30);
    assert!(rendered.contains("Power draw  118.4 W"));
    assert!(rendered.contains("Power draw  now 118.4 W"));
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
