use ratatui::{
    Frame, Terminal,
    backend::TestBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Sparkline, Wrap},
};

use crate::service::ServiceState;

use super::{Dashboard, DashboardDevice, DeviceState, MetricSample, Overlay};

pub fn render(frame: &mut Frame<'_>, dashboard: &Dashboard) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(24),
            Constraint::Percentage(30),
            Constraint::Percentage(46),
        ])
        .split(rows[0]);
    render_scopes(frame, dashboard, columns[0]);
    render_devices(frame, dashboard, columns[1]);
    render_details(frame, dashboard, columns[2]);
    let footer = Paragraph::new(
        "[o] On  [x] Off  [r] Reboot  [a] Action  [s] SSH  [/] Filter  [R] Refresh  [?] Help  [q] Quit",
    )
    .style(Style::default().fg(Color::Cyan));
    frame.render_widget(footer, rows[1]);
    let status = dashboard
        .message()
        .map_or_else(|| format!("filter: {}", dashboard.filter()), str::to_owned);
    frame.render_widget(Paragraph::new(status), rows[2]);
    render_overlay(frame, dashboard);
}

fn render_scopes(frame: &mut Frame<'_>, dashboard: &Dashboard, area: Rect) {
    let items = dashboard.scopes().map(|(name, selected)| {
        let marker = if selected { "> " } else { "  " };
        ListItem::new(format!("{marker}{name}"))
    });
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .title(" Sites / Groups ")
                .borders(Borders::ALL),
        ),
        area,
    );
}

fn render_devices(frame: &mut Frame<'_>, dashboard: &Dashboard, area: Rect) {
    let selected = dashboard.selected_device();
    let items = dashboard.visible_devices().into_iter().map(|name| {
        let device = dashboard.device(name).expect("visible device exists");
        let marker = if selected == Some(name) { ">" } else { " " };
        let busy = device
            .busy
            .as_deref()
            .map_or(String::new(), |operation| format!(" [{operation}…]"));
        let style = state_style(device.state);
        ListItem::new(Line::from(vec![
            Span::raw(format!("{marker} {name:<16} ")),
            Span::styled(device.state.label(), style),
            Span::raw(busy),
        ]))
    });
    frame.render_widget(
        List::new(items).block(Block::default().title(" Devices ").borders(Borders::ALL)),
        area,
    );
}

fn render_details(frame: &mut Frame<'_>, dashboard: &Dashboard, area: Rect) {
    let block = Block::default().title(" Details ").borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(device) = dashboard
        .selected_device()
        .and_then(|name| dashboard.device(name))
    else {
        frame.render_widget(Paragraph::new("No matching devices"), inner);
        return;
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(inner);
    frame.render_widget(Paragraph::new(detail_lines(device)), chunks[0]);
    sparkline(frame, " RAM ", &device.ram_history, chunks[1], 100);
    sparkline(frame, " GPU ", &device.gpu_history, chunks[2], 100);
    let watt_max = device
        .watts_history
        .iter()
        .map(|sample| sample.0.ceil() as u64)
        .max()
        .unwrap_or(1);
    sparkline(frame, " Watts ", &device.watts_history, chunks[3], watt_max);
    if let Some(error) = &device.recent_failure {
        frame.render_widget(
            Paragraph::new(format!("Recent failure: {error}"))
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: true }),
            chunks[4],
        );
    }
}

fn detail_lines(device: &DashboardDevice) -> Vec<Line<'static>> {
    let service = device.service_state.map_or_else(
        || "—".to_owned(),
        |state| {
            format!(
                "{} {}",
                device.service_name.as_deref().unwrap_or("service"),
                service_label(state)
            )
        },
    );
    vec![
        Line::from(vec![
            Span::styled(
                device.name.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  {}", device.state.label())),
        ]),
        Line::from(format!("SSH     {}", device.ssh)),
        Line::from(format!("RAM     {}", percent(device.ram_percent))),
        Line::from(format!("GPU     {}", percent(device.gpu_percent))),
        Line::from(format!(
            "Power   {}",
            device
                .watts
                .map_or_else(|| "—".to_owned(), |watts| format!("{watts:.0} W"))
        )),
        Line::from(format!("Service {service}")),
        Line::from(format!(
            "Model   {}",
            device.model.as_deref().unwrap_or("—")
        )),
    ]
}

fn percent(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| format!("{value:.0}%"))
}

fn service_label(state: ServiceState) -> &'static str {
    match state {
        ServiceState::Stopped => "stopped",
        ServiceState::Loading => "loading",
        ServiceState::Ready => "ready",
        ServiceState::Error => "error",
        ServiceState::Unknown => "unknown",
    }
}

fn state_style(state: DeviceState) -> Style {
    Style::default().fg(match state {
        DeviceState::Running => Color::Green,
        DeviceState::Booting => Color::Yellow,
        DeviceState::Off => Color::DarkGray,
        DeviceState::Unreachable => Color::Magenta,
        DeviceState::Error => Color::Red,
        DeviceState::Unknown => Color::Gray,
    })
}

fn sparkline(
    frame: &mut Frame<'_>,
    title: &str,
    values: &std::collections::VecDeque<MetricSample>,
    area: Rect,
    max: u64,
) {
    let values = values
        .iter()
        .map(|sample| sample.0.max(0.0).round() as u64)
        .collect::<Vec<_>>();
    frame.render_widget(
        Sparkline::default()
            .block(Block::default().title(title).borders(Borders::ALL))
            .data(&values)
            .max(max.max(1))
            .style(Style::default().fg(Color::Cyan)),
        area,
    );
}

fn render_overlay(frame: &mut Frame<'_>, dashboard: &Dashboard) {
    let (title, body) = match dashboard.overlay() {
        Overlay::None => return,
        Overlay::Help => (
            " Help ",
            "↑/k ↓/j navigate · ←/h →/l pane · / filter · R refresh\n\
             o on · x off · r reboot · a named action · s SSH handoff\n\
             Esc closes dialogs · q quits. Telemetry history is memory-only.",
        ),
        Overlay::Search => (" Filter ", dashboard.search_buffer()),
        Overlay::Actions => {
            let (actions, selected) = dashboard.actions();
            let body = actions
                .iter()
                .enumerate()
                .map(|(index, action)| {
                    format!("{} {action}", if index == selected { ">" } else { " " })
                })
                .collect::<Vec<_>>()
                .join("\n");
            render_popup(frame, " Named actions ", &body);
            return;
        }
        Overlay::Confirm {
            operation,
            target,
            devices,
        } => {
            let body = format!(
                "{} `{target}` on:\n{}\n\n[y/Enter] confirm  [n/Esc] cancel",
                operation.label(),
                devices.join(", ")
            );
            render_popup(frame, " Confirm operation ", &body);
            return;
        }
    };
    render_popup(frame, title, body);
}

fn render_popup(frame: &mut Frame<'_>, title: &str, body: &str) {
    let area = centered_rect(68, 45, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(body)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true })
            .block(Block::default().title(title).borders(Borders::ALL)),
        area,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - height) / 2),
        Constraint::Percentage(height),
        Constraint::Percentage((100 - height) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - width) / 2),
        Constraint::Percentage(width),
        Constraint::Percentage((100 - width) / 2),
    ])
    .split(vertical[1])[1]
}

pub fn render_to_string(dashboard: &Dashboard, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| render(frame, dashboard))
        .expect("render dashboard");
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
