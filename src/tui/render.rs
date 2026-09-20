use ratatui::{
    Frame, Terminal,
    backend::TestBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Sparkline, Wrap},
};

use crate::service::ServiceState;

use super::{Dashboard, DashboardDevice, DashboardService, DeviceState, MetricSample, Overlay};

const DETAIL_LABEL_WIDTH: usize = 12;

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
    let chart_height = if inner.height >= 24 { 5 } else { 4 };
    let show_power = device.watts.is_some() || !device.watts_history.is_empty();
    let details = detail_lines(device, show_power);
    let detail_height = details.len().min(10) as u16;
    let mut constraints = vec![
        Constraint::Length(detail_height),
        Constraint::Length(chart_height),
        Constraint::Length(1),
        Constraint::Length(chart_height),
    ];
    if show_power {
        constraints.push(Constraint::Length(chart_height));
    }
    constraints.push(Constraint::Min(1));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);
    frame.render_widget(Paragraph::new(details), chunks[0]);
    metric_history(frame, "RAM used", "%", &device.ram_history, chunks[1], None);
    metric_history(frame, "GPU busy", "%", &device.gpu_history, chunks[3], None);
    let error_chunk = if show_power {
        metric_history(
            frame,
            "Power draw",
            " W",
            &device.watts_history,
            chunks[4],
            None,
        );
        5
    } else {
        4
    };
    if let Some(error) = &device.recent_failure {
        frame.render_widget(
            Paragraph::new(format!("Recent failure: {error}"))
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: true }),
            chunks[error_chunk],
        );
    }
}

fn detail_lines(device: &DashboardDevice, show_power: bool) -> Vec<Line<'static>> {
    let mut lines = vec![
        detail_row(
            "Device",
            vec![
                Span::styled(
                    device.name.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("  {}", device.state.label())),
            ],
        ),
        detail_row("SSH", vec![Span::raw(device.ssh.clone())]),
        detail_row("RAM used", vec![Span::raw(memory_used(device))]),
        detail_row("GPU busy", vec![Span::raw(percent(device.gpu_percent))]),
    ];
    if show_power {
        lines.push(detail_row(
            "Power draw",
            vec![Span::raw(device.watts.map_or_else(
                || "—".to_owned(),
                |watts| format!("{watts:.1} W"),
            ))],
        ));
    }
    lines.extend(device.services.iter().map(service_line));
    lines.push(Line::default());
    lines
}

fn detail_row(label: &str, value: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{label:<DETAIL_LABEL_WIDTH$}"),
        Style::default().fg(Color::DarkGray),
    )];
    spans.extend(value);
    Line::from(spans)
}

fn percent(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| format!("{value:.1}%"))
}

fn memory_used(device: &DashboardDevice) -> String {
    match (
        device.ram_used_bytes,
        device.ram_total_bytes,
        device.ram_percent,
    ) {
        (Some(used), Some(total), Some(percent)) => format!(
            "{:.1} / {:.1} GiB ({percent:.1}%)",
            gibibytes(used),
            gibibytes(total)
        ),
        (_, _, percent) => self::percent(percent),
    }
}

fn gibibytes(bytes: u64) -> f64 {
    bytes as f64 / 1024_f64.powi(3)
}

fn service_line(service: &DashboardService) -> Line<'static> {
    let models = if service.models.is_empty() {
        String::new()
    } else {
        format!(" - {}", service.models.join(", "))
    };
    detail_row(
        &service.name,
        vec![Span::raw(format!(
            "{}{models}",
            service_label(service.state)
        ))],
    )
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

fn metric_history(
    frame: &mut Frame<'_>,
    title: &str,
    unit: &str,
    values: &std::collections::VecDeque<MetricSample>,
    area: Rect,
    empty_message: Option<&str>,
) {
    let Some(current) = values.back().map(|sample| sample.0) else {
        frame.render_widget(
            Paragraph::new(empty_message.unwrap_or("No samples yet")).block(
                Block::default()
                    .title(format!(" {title} "))
                    .borders(Borders::ALL),
            ),
            area,
        );
        return;
    };
    let minimum = values
        .iter()
        .map(|sample| sample.0)
        .fold(f64::INFINITY, f64::min);
    let maximum = values
        .iter()
        .map(|sample| sample.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let padding = ((maximum - minimum) * 0.1)
        .max(current.abs() * 0.005)
        .max(0.1);
    let scale_min = (minimum - padding).max(0.0);
    let scale_max = maximum + padding;
    let scale_range = (scale_max - scale_min).max(f64::EPSILON);
    let plotted = values
        .iter()
        .map(|sample| (((sample.0 - scale_min) / scale_range) * 1000.0).round() as u64)
        .collect::<Vec<_>>();
    let window = format_window(values.len());
    let title =
        format!(" {title}  now {current:.1}{unit}  min {minimum:.1}  max {maximum:.1}  {window} ");
    frame.render_widget(
        Sparkline::default()
            .block(Block::default().title(title).borders(Borders::ALL))
            .data(&plotted)
            .max(1000)
            .style(Style::default().fg(Color::Cyan)),
        area,
    );
}

fn format_window(samples: usize) -> String {
    let seconds = samples * 2;
    if seconds >= 60 {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
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
