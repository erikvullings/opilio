use std::{
    collections::HashSet,
    fs,
    io::{self, IsTerminal, Stdout},
    num::NonZeroUsize,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event as CrosstermEvent, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    action::{ActionReport, ActionRequest, ActionState, run_action},
    config::Config,
    domain::PowerProvider as ConfiguredPowerProvider,
    history::{HistoryResult, HistoryStore, NewHistoryRecord, OperationSource, Redactor},
    lifecycle::{
        LifecycleOperation, LifecycleReport, LifecycleRequest, LifecycleResultStatus,
        LifecycleState, SystemLifecycleExecutor, execute_lifecycle, plan_lifecycle,
    },
    power::{
        Metric as PowerMetric, OutletState, PowerProvider,
        shelly::{ReqwestHttpClient as ShellyHttpClient, ShellyProvider},
    },
    service::{
        EnvironmentSecretResolver, ReqwestHttpClient, ServiceCollector, ServiceObservation,
        ServiceState,
    },
    ssh::{CancellationToken, OpenSsh},
    telemetry::{
        Metric, ProviderSnapshot, TelemetryCollector, TelemetrySnapshot, nvidia::NvidiaMetrics,
        system::SystemMetrics,
    },
};

use super::{
    Dashboard, DashboardSample, DeviceState, Effect, Event, Key, Operation, PollKind, PollPolicy,
    render,
};

const EVENT_TICK: Duration = Duration::from_millis(100);
const POWER_TIMEOUT: Duration = Duration::from_secs(2);

enum RuntimeMessage {
    Poll(DashboardSample),
    Operation {
        device: String,
        operation: String,
        result: Result<DeviceState, String>,
    },
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    active: bool,
}

impl TerminalSession {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        let terminal = match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = disable_raw_mode();
                return Err(error);
            }
        };
        Ok(Self {
            terminal,
            active: true,
        })
    }

    fn suspend(&mut self) -> io::Result<()> {
        if self.active {
            disable_raw_mode()?;
            execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
            self.terminal.show_cursor()?;
            self.active = false;
        }
        Ok(())
    }

    fn resume(&mut self) -> io::Result<()> {
        if !self.active {
            enable_raw_mode()?;
            execute!(self.terminal.backend_mut(), EnterAlternateScreen)?;
            self.terminal.clear()?;
            self.active = true;
        }
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.active {
            let _ = disable_raw_mode();
            let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
            let _ = self.terminal.show_cursor();
        }
    }
}

struct BackgroundJobs {
    cancellation: CancellationToken,
    handles: Vec<JoinHandle<()>>,
}

impl BackgroundJobs {
    fn new() -> Self {
        Self {
            cancellation: CancellationToken::new(),
            handles: Vec::new(),
        }
    }

    fn spawn(&mut self, job: impl FnOnce(CancellationToken) + Send + 'static) {
        let cancellation = self.cancellation.clone();
        self.handles.push(thread::spawn(move || job(cancellation)));
    }

    fn reap(&mut self) {
        let mut index = 0;
        while index < self.handles.len() {
            if self.handles[index].is_finished() {
                let handle = self.handles.swap_remove(index);
                let _ = handle.join();
            } else {
                index += 1;
            }
        }
    }
}

impl Drop for BackgroundJobs {
    fn drop(&mut self) {
        self.cancellation.cancel();
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

pub fn run(config: Config) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other("the TUI requires an interactive terminal"));
    }
    let mut jobs = BackgroundJobs::new();
    let mut terminal = TerminalSession::enter()?;
    let mut dashboard = Dashboard::from_config(&config);
    let mut policy = PollPolicy::new(Instant::now());
    let (sender, receiver) = mpsc::channel();
    let mut polls_in_flight = HashSet::new();

    loop {
        drain_messages(&mut dashboard, &mut policy, &receiver, &mut polls_in_flight);
        jobs.reap();
        let now = Instant::now();
        let due = policy.due(now);
        if !due.is_empty() {
            for kind in &due {
                policy.mark_polled(*kind, now);
            }
            spawn_due_polls(
                &config,
                &dashboard,
                &policy,
                &due,
                &sender,
                &mut jobs,
                &mut polls_in_flight,
            );
        }
        terminal.terminal.draw(|frame| render(frame, &dashboard))?;

        if event::poll(EVENT_TICK)?
            && let CrosstermEvent::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            let effects = dashboard.update(Event::Key(map_key(key.code)));
            for effect in effects {
                match effect {
                    Effect::Quit => {
                        terminal.suspend()?;
                        return Ok(());
                    }
                    Effect::PollNow => policy.manual_refresh(Instant::now()),
                    Effect::Ssh { device } => {
                        terminal.suspend()?;
                        let result = interactive_ssh(&config, &device);
                        terminal.resume()?;
                        dashboard.update(Event::OperationFinished {
                            device,
                            operation: "ssh".to_owned(),
                            result,
                        });
                    }
                    Effect::Operate { operation, target } => {
                        spawn_operation(
                            config.clone(),
                            operation,
                            target,
                            sender.clone(),
                            &mut jobs,
                        );
                    }
                    Effect::RunAction { name, target } => {
                        spawn_action(config.clone(), name, target, sender.clone(), &mut jobs);
                    }
                }
            }
        }
    }
}

fn spawn_due_polls(
    config: &Config,
    dashboard: &Dashboard,
    policy: &PollPolicy,
    due: &[PollKind],
    sender: &Sender<RuntimeMessage>,
    jobs: &mut BackgroundJobs,
    polls_in_flight: &mut HashSet<String>,
) {
    for name in dashboard.visible_devices() {
        let site = config.devices()[name].site.as_deref().unwrap_or("");
        if !policy.site_due(site, Instant::now()) || !polls_in_flight.insert(name.to_owned()) {
            continue;
        }
        let config = config.clone();
        let name = name.to_owned();
        let due = due.to_vec();
        let sender = sender.clone();
        jobs.spawn(move |cancellation| {
            let sample = poll_device(&config, &name, &due, &cancellation);
            let _ = sender.send(RuntimeMessage::Poll(sample));
        });
    }
}

fn poll_device(
    config: &Config,
    name: &str,
    due: &[PollKind],
    cancellation: &CancellationToken,
) -> DashboardSample {
    let device = &config.devices()[name];
    let power = poll_power(device);
    let mut sample = DashboardSample {
        device: name.to_owned(),
        state: DeviceState::Unknown,
        ram_percent: None,
        gpu_percent: None,
        watts: power.as_ref().and_then(|(_, watts, _)| *watts),
        service: None,
        error: power.as_ref().and_then(|(_, _, error)| error.clone()),
    };
    let ssh = match OpenSsh::system() {
        Ok(ssh) => ssh,
        Err(error) => {
            sample.state = DeviceState::Error;
            sample.error = Some(error.to_string());
            return sample;
        }
    };
    let path = control_path(name);
    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        sample.state = DeviceState::Error;
        sample.error = Some(format!("could not prepare SSH control directory: {error}"));
        return sample;
    }
    let master = match ssh.start_control_master(&device.ssh, &path) {
        Ok(master) => master,
        Err(error) => {
            let _ = fs::remove_file(path);
            sample.state = if power
                .as_ref()
                .is_some_and(|(outlet, _, _)| *outlet == OutletState::Off)
            {
                DeviceState::Off
            } else {
                DeviceState::Unreachable
            };
            sample.error = Some(error.to_string());
            return sample;
        }
    };
    sample.state = DeviceState::Running;
    if due.contains(&PollKind::Telemetry)
        && let Some(provider) = device.telemetry.clone()
    {
        apply_telemetry(
            &mut sample,
            TelemetryCollector::new(&master).collect(
                &device.ssh,
                &device.shell,
                provider,
                cancellation,
            ),
        );
    }
    if due.contains(&PollKind::Service) && !device.services.is_empty() {
        let http = match ReqwestHttpClient::new() {
            Ok(http) => http,
            Err(error) => {
                sample.error = Some(error);
                return sample;
            }
        };
        let secrets = EnvironmentSecretResolver;
        let collector = ServiceCollector::new(&master, &http, &secrets);
        let observations = device
            .services
            .iter()
            .map(|service| {
                collector.collect(service, &config.services()[service], device, cancellation)
            })
            .collect::<Vec<_>>();
        apply_services(&mut sample, &observations);
    }
    let _ = master.close();
    let _ = fs::remove_file(path);
    sample
}

fn poll_power(
    device: &crate::domain::Device,
) -> Option<(OutletState, Option<f64>, Option<String>)> {
    if !matches!(device.power, Some(ConfiguredPowerProvider::Shelly { .. })) {
        return None;
    }
    match ShellyProvider::<ShellyHttpClient>::from_device(device, POWER_TIMEOUT) {
        Ok(provider) => {
            let status = provider.status();
            let watts = match status.telemetry.power_watts {
                PowerMetric::Value(value) => Some(value),
                PowerMetric::Unsupported | PowerMetric::Unknown => None,
            };
            Some((status.outlet, watts, status.error))
        }
        Err(error) => Some((OutletState::Unknown, None, Some(error.to_string()))),
    }
}

fn apply_telemetry(sample: &mut DashboardSample, telemetry: TelemetrySnapshot) {
    if let ProviderSnapshot::Available { data } = telemetry.system {
        sample.ram_percent = memory_percent(&data);
    }
    if let Some(ProviderSnapshot::Available { data }) = telemetry.nvidia {
        sample.gpu_percent = gpu_percent(&data);
    }
}

fn memory_percent(metrics: &SystemMetrics) -> Option<f64> {
    match (&metrics.memory.total_bytes, &metrics.memory.available_bytes) {
        (Metric::Available { value: total }, Metric::Available { value: available })
            if *total > 0 =>
        {
            Some((1.0 - (*available as f64 / *total as f64)) * 100.0)
        }
        _ => None,
    }
}

fn gpu_percent(metrics: &NvidiaMetrics) -> Option<f64> {
    metrics
        .gpus
        .iter()
        .find_map(|gpu| match gpu.utilization_percent {
            Metric::Available { value } => Some(value),
            Metric::Unsupported | Metric::Unavailable => None,
        })
}

fn apply_services(sample: &mut DashboardSample, observations: &[ServiceObservation]) {
    let Some(observation) = observations.first() else {
        return;
    };
    let model = observation
        .fields
        .get("model")
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    sample.service = Some((observation.name.clone(), observation.state, model));
    if observation.state == ServiceState::Error {
        sample.error = Some(format!("service `{}` reported an error", observation.name));
    }
}

fn spawn_operation(
    config: Config,
    operation: Operation,
    target: String,
    sender: Sender<RuntimeMessage>,
    jobs: &mut BackgroundJobs,
) {
    jobs.spawn(move |_| {
        let started = Instant::now();
        let lifecycle = match operation {
            Operation::On => LifecycleOperation::On,
            Operation::Off => LifecycleOperation::Off,
            Operation::Reboot => LifecycleOperation::Reboot,
        };
        let request = LifecycleRequest {
            operation: lifecycle,
            target,
            confirmed: true,
            force: false,
            wait: false,
            parallelism: NonZeroUsize::MIN,
        };
        let result = plan_lifecycle(&config, request)
            .and_then(|plan| execute_lifecycle(&config, plan, &SystemLifecycleExecutor::system()));
        match result {
            Ok(report) => {
                record_lifecycle_history(&config, &report, started.elapsed());
                for result in report.devices {
                    let state = result
                        .error
                        .map_or_else(|| Ok(map_lifecycle_state(result.state)), Err);
                    let _ = sender.send(RuntimeMessage::Operation {
                        device: result.device,
                        operation: operation.label().to_owned(),
                        result: state,
                    });
                }
            }
            Err(error) => {
                let _ = sender.send(RuntimeMessage::Operation {
                    device: "selection".to_owned(),
                    operation: operation.label().to_owned(),
                    result: Err(error.to_string()),
                });
            }
        }
    });
}

fn spawn_action(
    config: Config,
    name: String,
    target: String,
    sender: Sender<RuntimeMessage>,
    jobs: &mut BackgroundJobs,
) {
    jobs.spawn(move |_| {
        let started = Instant::now();
        let operation = format!("action {name}");
        let result = OpenSsh::system()
            .map_err(|error| error.to_string())
            .and_then(|ssh| {
                run_action(
                    &config,
                    ActionRequest {
                        name,
                        target,
                        parallelism: NonZeroUsize::MIN,
                    },
                    &ssh,
                )
                .map_err(|error| error.to_string())
            });
        match result {
            Ok(report) => {
                record_action_history(&config, &report, started.elapsed());
                for result in report.devices {
                    let outcome = result.error.map_or_else(|| Ok(DeviceState::Running), Err);
                    let _ = sender.send(RuntimeMessage::Operation {
                        device: result.device,
                        operation: operation.clone(),
                        result: outcome,
                    });
                }
            }
            Err(error) => {
                let _ = sender.send(RuntimeMessage::Operation {
                    device: "selection".to_owned(),
                    operation,
                    result: Err(error),
                });
            }
        }
    });
}

fn history_store(config: &Config) -> Option<HistoryStore> {
    HistoryStore::platform_default(Redactor::new(config.resolved_secret_values())).ok()
}

fn record_lifecycle_history(config: &Config, report: &LifecycleReport, duration: Duration) {
    let Some(history) = history_store(config) else {
        return;
    };
    for result in &report.devices {
        let failed = result.status == LifecycleResultStatus::Failed;
        let _ = history.append(NewHistoryRecord {
            source: OperationSource::Tui,
            operation: report.operation.to_string(),
            action: None,
            requested_target: report.target.clone(),
            resolved_device: result.device.clone(),
            duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
            result: if failed {
                HistoryResult::Failed
            } else {
                HistoryResult::Succeeded
            },
            exit_code: Some(i32::from(failed)),
            force: false,
            stdout: None,
            stderr: None,
            error: result.error.clone(),
        });
    }
}

fn record_action_history(config: &Config, report: &ActionReport, duration: Duration) {
    let Some(history) = history_store(config) else {
        return;
    };
    for result in &report.devices {
        let failed = result.status == ActionState::Failed;
        let _ = history.append(NewHistoryRecord {
            source: OperationSource::Tui,
            operation: "action".to_owned(),
            action: Some(report.action.clone()),
            requested_target: report.target.clone(),
            resolved_device: result.device.clone(),
            duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
            result: if failed {
                HistoryResult::Failed
            } else {
                HistoryResult::Succeeded
            },
            exit_code: result.exit_code,
            force: false,
            stdout: failed.then(|| result.stdout.clone()),
            stderr: failed.then(|| result.stderr.clone()),
            error: result.error.clone(),
        });
    }
}

fn drain_messages(
    dashboard: &mut Dashboard,
    policy: &mut PollPolicy,
    receiver: &Receiver<RuntimeMessage>,
    polls_in_flight: &mut HashSet<String>,
) {
    while let Ok(message) = receiver.try_recv() {
        match message {
            RuntimeMessage::Poll(sample) => {
                polls_in_flight.remove(&sample.device);
                let site = dashboard
                    .device(&sample.device)
                    .and_then(|device| device.site.clone())
                    .unwrap_or_default();
                policy.record_site_result(
                    &site,
                    matches!(sample.state, DeviceState::Running | DeviceState::Booting),
                    Instant::now(),
                );
                dashboard.update(Event::PollCompleted(sample));
            }
            RuntimeMessage::Operation {
                device,
                operation,
                result,
            } => {
                dashboard.update(Event::OperationFinished {
                    device,
                    operation,
                    result,
                });
                policy.manual_refresh(Instant::now());
            }
        }
    }
}

fn interactive_ssh(config: &Config, device: &str) -> Result<DeviceState, String> {
    let target = config
        .devices()
        .get(device)
        .ok_or_else(|| format!("unknown device `{device}`"))?;
    let started = Instant::now();
    let result = OpenSsh::system()
        .map_err(|error| error.to_string())
        .and_then(|ssh| {
            ssh.interactive(&target.ssh)
                .map_err(|error| error.to_string())
        })
        .and_then(|code| {
            (code == 0)
                .then_some(DeviceState::Running)
                .ok_or_else(|| format!("SSH exited with code {code}"))
        });
    if let Some(history) = history_store(config) {
        let failed = result.is_err();
        let _ = history.append(NewHistoryRecord {
            source: OperationSource::Tui,
            operation: "ssh".to_owned(),
            action: None,
            requested_target: device.to_owned(),
            resolved_device: device.to_owned(),
            duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            result: if failed {
                HistoryResult::Failed
            } else {
                HistoryResult::Succeeded
            },
            exit_code: Some(i32::from(failed)),
            force: false,
            stdout: None,
            stderr: None,
            error: result.as_ref().err().cloned(),
        });
    }
    result
}

fn map_lifecycle_state(state: LifecycleState) -> DeviceState {
    match state {
        LifecycleState::Booting | LifecycleState::Rebooting => DeviceState::Booting,
        LifecycleState::SshReady => DeviceState::Running,
        LifecycleState::PoweredOff => DeviceState::Off,
        LifecycleState::Unreachable => DeviceState::Unreachable,
        LifecycleState::ShuttingDown => DeviceState::Unknown,
        LifecycleState::Unknown => DeviceState::Unknown,
    }
}

fn control_path(device: &str) -> PathBuf {
    let base = directories::ProjectDirs::from("", "", "opilio")
        .map(|dirs| dirs.cache_dir().to_owned())
        .unwrap_or_else(|| PathBuf::from(".opilio-cache"));
    let safe = device
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    base.join("ssh")
        .join(format!("{}-{safe}.sock", std::process::id()))
}

fn map_key(code: KeyCode) -> Key {
    match code {
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Escape,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Char('/') => Key::Search,
        KeyCode::Char('R') => Key::Refresh,
        KeyCode::Char('o') => Key::On,
        KeyCode::Char('x') => Key::Off,
        KeyCode::Char('r') => Key::Reboot,
        KeyCode::Char('a') => Key::Action,
        KeyCode::Char('s') => Key::Ssh,
        KeyCode::Char('?') => Key::Help,
        KeyCode::Char('q') => Key::Quit,
        KeyCode::Char(character) => Key::Char(character),
        _ => Key::Char('\0'),
    }
}
