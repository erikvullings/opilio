use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    time::{Duration, Instant},
};

use crate::{config::Config, service::ServiceState};

const HISTORY_CAPACITY: usize = 60;
const TELEMETRY_INTERVAL: Duration = Duration::from_secs(2);
const SERVICE_INTERVAL: Duration = Duration::from_secs(7);
const SITE_BACKOFF: [Duration; 3] = [
    Duration::from_secs(10),
    Duration::from_secs(20),
    Duration::from_secs(30),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Off,
    Unreachable,
    Running,
    Booting,
    Error,
    Unknown,
}

impl DeviceState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Unreachable => "unreachable",
            Self::Running => "running",
            Self::Booting => "booting",
            Self::Error => "error",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricSample(pub f64);

#[derive(Debug, Clone)]
pub struct DashboardDevice {
    pub name: String,
    pub label: Option<String>,
    pub site: Option<String>,
    pub groups: Vec<String>,
    pub ssh: String,
    pub state: DeviceState,
    pub ram_percent: Option<f64>,
    pub ram_used_bytes: Option<u64>,
    pub ram_total_bytes: Option<u64>,
    pub gpu_percent: Option<f64>,
    pub watts: Option<f64>,
    pub ram_history: VecDeque<MetricSample>,
    pub gpu_history: VecDeque<MetricSample>,
    pub watts_history: VecDeque<MetricSample>,
    pub services: Vec<DashboardService>,
    pub recent_failure: Option<String>,
    pub busy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardService {
    pub name: String,
    pub state: ServiceState,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardSample {
    pub device: String,
    pub state: DeviceState,
    pub ram_percent: Option<f64>,
    pub ram_used_bytes: Option<u64>,
    pub ram_total_bytes: Option<u64>,
    pub gpu_percent: Option<f64>,
    pub watts: Option<f64>,
    pub services: Option<Vec<DashboardService>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    On,
    Off,
    Reboot,
}

impl Operation {
    pub const fn label(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Reboot => "reboot",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Quit,
    PollNow,
    Operate {
        operation: Operation,
        target: String,
    },
    RunAction {
        name: String,
        target: String,
    },
    Ssh {
        device: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Backspace,
    Char(char),
    Search,
    Refresh,
    On,
    Off,
    Reboot,
    Action,
    Ssh,
    Help,
    Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Key(Key),
    PollCompleted(DashboardSample),
    OperationFinished {
        device: String,
        operation: String,
        result: Result<DeviceState, String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    None,
    Help,
    Search,
    Actions,
    Confirm {
        operation: Operation,
        target: String,
        devices: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Scopes,
    Devices,
}

#[derive(Debug, Clone)]
enum ScopeKind {
    All,
    Site(String),
    Group(String),
}

#[derive(Debug, Clone)]
struct Scope {
    title: String,
    target: String,
    kind: ScopeKind,
}

#[derive(Debug, Clone)]
pub struct Dashboard {
    scopes: Vec<Scope>,
    devices: BTreeMap<String, DashboardDevice>,
    actions: Vec<String>,
    selected_scope: usize,
    selected_device: usize,
    selected_action: usize,
    focus: Focus,
    filter: String,
    search_buffer: String,
    overlay: Overlay,
    message: Option<String>,
}

impl Dashboard {
    pub fn from_config(config: &Config) -> Self {
        let mut scopes = vec![Scope {
            title: "All devices".to_owned(),
            target: "all".to_owned(),
            kind: ScopeKind::All,
        }];
        scopes.extend(config.sites().iter().map(|(name, site)| Scope {
            title: format!("Site: {}", site.label.as_deref().unwrap_or(name.as_str())),
            target: name.clone(),
            kind: ScopeKind::Site(name.clone()),
        }));
        scopes.extend(config.groups().keys().map(|name| Scope {
            title: format!("Group: {name}"),
            target: name.clone(),
            kind: ScopeKind::Group(name.clone()),
        }));
        let devices = config
            .devices()
            .iter()
            .map(|(name, device)| {
                (
                    name.clone(),
                    DashboardDevice {
                        name: name.clone(),
                        label: device.label.clone(),
                        site: device.site.clone(),
                        groups: device.groups.clone(),
                        ssh: device.ssh.clone(),
                        state: DeviceState::Unknown,
                        ram_percent: None,
                        ram_used_bytes: None,
                        ram_total_bytes: None,
                        gpu_percent: None,
                        watts: None,
                        ram_history: VecDeque::with_capacity(HISTORY_CAPACITY),
                        gpu_history: VecDeque::with_capacity(HISTORY_CAPACITY),
                        watts_history: VecDeque::with_capacity(HISTORY_CAPACITY),
                        services: Vec::new(),
                        recent_failure: None,
                        busy: None,
                    },
                )
            })
            .collect();
        Self {
            scopes,
            devices,
            actions: config.actions().keys().cloned().collect(),
            selected_scope: 0,
            selected_device: 0,
            selected_action: 0,
            focus: Focus::Devices,
            filter: String::new(),
            search_buffer: String::new(),
            overlay: Overlay::None,
            message: None,
        }
    }

    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::PollCompleted(sample) => {
                self.apply_sample(sample);
                Vec::new()
            }
            Event::OperationFinished {
                device,
                operation,
                result,
            } => {
                if let Some(item) = self.devices.get_mut(&device) {
                    item.busy = None;
                    match result {
                        Ok(state) => item.state = state,
                        Err(error) => {
                            item.state = DeviceState::Error;
                            item.recent_failure = Some(error);
                        }
                    }
                }
                self.message = Some(format!("{operation} finished for {device}"));
                Vec::new()
            }
            Event::Key(key) => self.update_key(key),
        }
    }

    fn update_key(&mut self, key: Key) -> Vec<Effect> {
        match &self.overlay {
            Overlay::Search => return self.update_search(key),
            Overlay::Help => {
                if matches!(key, Key::Escape | Key::Help | Key::Quit) {
                    self.overlay = Overlay::None;
                }
                return Vec::new();
            }
            Overlay::Actions => return self.update_actions(key),
            Overlay::Confirm { .. } => return self.update_confirmation(key),
            Overlay::None => {}
        }
        match key {
            Key::Quit | Key::Char('q') => vec![Effect::Quit],
            Key::Up | Key::Char('k') => {
                self.move_selection(-1);
                Vec::new()
            }
            Key::Down | Key::Char('j') => {
                self.move_selection(1);
                Vec::new()
            }
            Key::Left | Key::Char('h') => {
                self.focus = Focus::Scopes;
                Vec::new()
            }
            Key::Right | Key::Char('l') | Key::Enter => {
                self.focus = Focus::Devices;
                Vec::new()
            }
            Key::Search => {
                self.search_buffer = self.filter.clone();
                self.overlay = Overlay::Search;
                Vec::new()
            }
            Key::Refresh => vec![Effect::PollNow],
            Key::Help => {
                self.overlay = Overlay::Help;
                Vec::new()
            }
            Key::Action => {
                if !self.actions.is_empty() {
                    self.overlay = Overlay::Actions;
                }
                Vec::new()
            }
            Key::Ssh => self
                .selected_device()
                .map(|device| Effect::Ssh {
                    device: device.to_owned(),
                })
                .into_iter()
                .collect(),
            Key::On => self.operation(Operation::On),
            Key::Off => self.operation(Operation::Off),
            Key::Reboot => self.operation(Operation::Reboot),
            Key::Escape | Key::Backspace | Key::Char(_) => Vec::new(),
        }
    }

    fn update_search(&mut self, key: Key) -> Vec<Effect> {
        match key {
            Key::Enter => {
                self.filter = self.search_buffer.clone();
                self.selected_device = 0;
                self.overlay = Overlay::None;
            }
            Key::Escape => self.overlay = Overlay::None,
            Key::Backspace => {
                self.search_buffer.pop();
            }
            Key::Char(character) => self.search_buffer.push(character),
            _ => {}
        }
        Vec::new()
    }

    fn update_actions(&mut self, key: Key) -> Vec<Effect> {
        match key {
            Key::Escape | Key::Quit => self.overlay = Overlay::None,
            Key::Up | Key::Char('k') => {
                self.selected_action = self.selected_action.saturating_sub(1);
            }
            Key::Down | Key::Char('j') => {
                self.selected_action =
                    (self.selected_action + 1).min(self.actions.len().saturating_sub(1));
            }
            Key::Enter => {
                let Some(name) = self.actions.get(self.selected_action).cloned() else {
                    return Vec::new();
                };
                let target = self.current_target();
                self.overlay = Overlay::None;
                self.mark_busy(&target, &name);
                return vec![Effect::RunAction { name, target }];
            }
            _ => {}
        }
        Vec::new()
    }

    fn update_confirmation(&mut self, key: Key) -> Vec<Effect> {
        let Overlay::Confirm {
            operation, target, ..
        } = self.overlay.clone()
        else {
            return Vec::new();
        };
        match key {
            Key::Char('y') | Key::Enter => {
                self.overlay = Overlay::None;
                self.mark_busy(&target, operation.label());
                vec![Effect::Operate { operation, target }]
            }
            Key::Char('n') | Key::Escape | Key::Quit => {
                self.overlay = Overlay::None;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn operation(&mut self, operation: Operation) -> Vec<Effect> {
        let target = self.current_target();
        if self.focus == Focus::Scopes {
            let devices = self.visible_devices_owned();
            self.overlay = Overlay::Confirm {
                operation,
                target,
                devices,
            };
            Vec::new()
        } else if let Some(device) = self.selected_device().map(str::to_owned) {
            self.mark_busy(&device, operation.label());
            vec![Effect::Operate {
                operation,
                target: device,
            }]
        } else {
            Vec::new()
        }
    }

    fn move_selection(&mut self, delta: isize) {
        match self.focus {
            Focus::Scopes => {
                self.selected_scope = move_index(self.selected_scope, self.scopes.len(), delta);
                self.selected_device = 0;
            }
            Focus::Devices => {
                self.selected_device =
                    move_index(self.selected_device, self.visible_devices().len(), delta);
            }
        }
    }

    fn mark_busy(&mut self, target: &str, operation: &str) {
        let affected = if self.devices.contains_key(target) {
            vec![target.to_owned()]
        } else {
            self.visible_devices_owned()
        };
        for name in affected {
            if let Some(device) = self.devices.get_mut(&name) {
                device.busy = Some(operation.to_owned());
                if operation == "on" {
                    device.state = DeviceState::Booting;
                }
            }
        }
    }

    fn apply_sample(&mut self, sample: DashboardSample) {
        let Some(device) = self.devices.get_mut(&sample.device) else {
            return;
        };
        device.state = sample.state;
        if sample.ram_percent.is_some() {
            device.ram_percent = sample.ram_percent;
        }
        if sample.ram_used_bytes.is_some() {
            device.ram_used_bytes = sample.ram_used_bytes;
        }
        if sample.ram_total_bytes.is_some() {
            device.ram_total_bytes = sample.ram_total_bytes;
        }
        if sample.gpu_percent.is_some() {
            device.gpu_percent = sample.gpu_percent;
        }
        if sample.watts.is_some() {
            device.watts = sample.watts;
        }
        push_metric(&mut device.ram_history, sample.ram_percent);
        push_metric(&mut device.gpu_history, sample.gpu_percent);
        push_metric(&mut device.watts_history, sample.watts);
        if let Some(services) = sample.services {
            device.services = services;
        }
        if let Some(error) = sample.error {
            device.recent_failure = Some(error);
        }
    }

    fn current_target(&self) -> String {
        if self.focus == Focus::Devices {
            self.selected_device()
                .unwrap_or_else(|| self.selected_scope_name())
                .to_owned()
        } else {
            self.scopes[self.selected_scope].target.clone()
        }
    }

    pub fn selected_device(&self) -> Option<&str> {
        self.visible_devices().get(self.selected_device).copied()
    }

    pub fn selected_scope_name(&self) -> &str {
        &self.scopes[self.selected_scope].target
    }

    pub fn visible_devices(&self) -> Vec<&str> {
        let scope = &self.scopes[self.selected_scope].kind;
        let filter = self.filter.to_ascii_lowercase();
        self.devices
            .values()
            .filter(|device| match scope {
                ScopeKind::All => true,
                ScopeKind::Site(site) => device.site.as_ref() == Some(site),
                ScopeKind::Group(group) => device.groups.contains(group),
            })
            .filter(|device| {
                filter.is_empty()
                    || device.name.to_ascii_lowercase().contains(&filter)
                    || device
                        .label
                        .as_deref()
                        .is_some_and(|label| label.to_ascii_lowercase().contains(&filter))
            })
            .map(|device| device.name.as_str())
            .collect()
    }

    fn visible_devices_owned(&self) -> Vec<String> {
        self.visible_devices()
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    pub fn device(&self, name: &str) -> Option<&DashboardDevice> {
        self.devices.get(name)
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn overlay(&self) -> &Overlay {
        &self.overlay
    }

    pub fn busy(&self) -> bool {
        self.devices.values().any(|device| device.busy.is_some())
    }

    pub(crate) fn scopes(&self) -> impl Iterator<Item = (&str, bool)> {
        self.scopes
            .iter()
            .enumerate()
            .map(|(index, scope)| (scope.title.as_str(), index == self.selected_scope))
    }

    pub(crate) fn actions(&self) -> (&[String], usize) {
        (&self.actions, self.selected_action)
    }

    pub(crate) fn search_buffer(&self) -> &str {
        &self.search_buffer
    }

    pub(crate) fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

fn move_index(index: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    index.saturating_add_signed(delta).min(len - 1)
}

fn push_metric(history: &mut VecDeque<MetricSample>, value: Option<f64>) {
    if let Some(value) = value.filter(|value| value.is_finite()) {
        if history.len() == HISTORY_CAPACITY {
            history.pop_front();
        }
        history.push_back(MetricSample(value));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollKind {
    Telemetry,
    Service,
}

#[derive(Debug, Clone)]
struct SitePoll {
    failures: usize,
    next: Instant,
}

#[derive(Debug, Clone)]
pub struct PollPolicy {
    next_telemetry: Instant,
    next_service: Instant,
    sites: HashMap<String, SitePoll>,
}

impl PollPolicy {
    pub fn new(now: Instant) -> Self {
        Self {
            next_telemetry: now,
            next_service: now,
            sites: HashMap::new(),
        }
    }

    pub fn due(&self, now: Instant) -> Vec<PollKind> {
        let mut due = Vec::new();
        if now >= self.next_telemetry {
            due.push(PollKind::Telemetry);
        }
        if now >= self.next_service {
            due.push(PollKind::Service);
        }
        due
    }

    pub fn mark_polled(&mut self, kind: PollKind, now: Instant) {
        match kind {
            PollKind::Telemetry => self.next_telemetry = now + TELEMETRY_INTERVAL,
            PollKind::Service => self.next_service = now + SERVICE_INTERVAL,
        }
    }

    pub fn record_site_result(&mut self, site: &str, reachable: bool, now: Instant) {
        let poll = self.sites.entry(site.to_owned()).or_insert(SitePoll {
            failures: 0,
            next: now,
        });
        if reachable {
            poll.failures = 0;
            poll.next = now + TELEMETRY_INTERVAL;
        } else {
            poll.failures = poll.failures.saturating_add(1);
            let index = poll.failures.saturating_sub(1).min(SITE_BACKOFF.len() - 1);
            poll.next = now + SITE_BACKOFF[index];
        }
    }

    pub fn site_delay(&self, site: &str) -> Duration {
        self.sites.get(site).map_or(TELEMETRY_INTERVAL, |poll| {
            if poll.failures == 0 {
                TELEMETRY_INTERVAL
            } else {
                SITE_BACKOFF[poll.failures.saturating_sub(1).min(SITE_BACKOFF.len() - 1)]
            }
        })
    }

    pub fn site_due(&self, site: &str, now: Instant) -> bool {
        self.sites.get(site).is_none_or(|poll| now >= poll.next)
    }

    pub fn manual_refresh(&mut self, now: Instant) {
        self.next_telemetry = now;
        self.next_service = now;
        for site in self.sites.values_mut() {
            site.next = now;
        }
    }
}
