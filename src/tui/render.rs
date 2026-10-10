use ratatui::{
    Frame, Terminal,
    backend::TestBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Sparkline, Wrap},
};

use crate::power::OutletState;
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
    let focused = dashboard.scopes_focused();
    let items = dashboard.scopes().map(|(name, selected)| {
        let marker = if selected { "> " } else { "  " };
        ListItem::new(format!("{marker}{name}")).style(selection_style(focused && selected))
    });
    frame.render_widget(
        List::new(items).block(panel_block("Sites / Groups", focused)),
        area,
    );
}

fn render_devices(frame: &mut Frame<'_>, dashboard: &Dashboard, area: Rect) {
    let focused = dashboard.devices_focused();
    let selected = dashboard.selected_device();
    let items = dashboard.visible_devices().into_iter().map(|name| {
        let device = dashboard.device(name).expect("visible device exists");
        let selected = selected == Some(name);
        let marker = if selected { ">" } else { " " };
        let busy = device
            .busy
            .as_deref()
            .map_or(String::new(), |operation| format!(" [{operation}…]"));
        let style = if selected && focused {
            selection_style(true)
        } else {
            state_style(device.state)
        };
        ListItem::new(Line::from(vec![
            Span::raw(format!("{marker} {name:<16} ")),
            Span::styled(device.state.label(), style),
            Span::raw(busy),
        ]))
        .style(selection_style(focused && selected))
    });
    frame.render_widget(
        List::new(items).block(panel_block("Devices", focused)),
        area,
    );
}

fn panel_block(title: &str, focused: bool) -> Block<'_> {
    let title = if focused {
        format!(" {title} [focus] ")
    } else {
        format!(" {title} ")
    };
    let block = Block::default().title(title).borders(Borders::ALL);
    if focused {
        block
            .border_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_type(ratatui::widgets::BorderType::Thick)
    } else {
        block
    }
}

fn selection_style(selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
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
    let mut messages = Vec::new();
    if let Some(error) = &device.recent_failure {
        messages.push(
            Line::from(format!("Recent failure: {error}")).style(Style::default().fg(Color::Red)),
        );
    }
    if let Some(error) = &device.status_detail {
        messages.push(
            Line::from(format!("Status detail: {error}")).style(Style::default().fg(Color::Yellow)),
        );
    }
    let error_height = messages
        .iter()
        .map(|line| line.width().div_ceil(inner.width.max(1) as usize) + 1)
        .sum::<usize>()
        .min(inner.height.saturating_sub(detail_height.saturating_add(2)) as usize)
        as u16;
    let errors = Paragraph::new(messages).wrap(Wrap { trim: true });
    let sections =
        Layout::vertical([Constraint::Min(0), Constraint::Length(error_height)]).split(inner);
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
        .split(sections[0]);
    frame.render_widget(Paragraph::new(details), chunks[0]);
    let ram_used = device
        .ram_used_bytes
        .map(|bytes| format!("{:.1} GiB", gibibytes(bytes)));
    metric_history(
        frame,
        "RAM used",
        "%",
        &device.ram_history,
        chunks[1],
        ram_used.as_deref(),
        Some((0.0, 100.0)),
    );
    metric_history(
        frame,
        "GPU busy",
        "%",
        &device.gpu_history,
        chunks[3],
        None,
        Some((0.0, 100.0)),
    );
    if show_power {
        metric_history(
            frame,
            "Power draw",
            " W",
            &device.watts_history,
            chunks[4],
            None,
            None,
        );
    }
    frame.render_widget(errors, sections[1]);
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
    if let Some(outlet) = device.outlet {
        lines.push(detail_row(
            "Outlet",
            vec![Span::styled(
                match outlet {
                    OutletState::On => "on",
                    OutletState::Off => "off",
                    OutletState::Unknown => "unknown",
                },
                Style::default().fg(match outlet {
                    OutletState::On => Color::Green,
                    OutletState::Off => Color::DarkGray,
                    OutletState::Unknown => Color::Yellow,
                }),
            )],
        ));
    }
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
    current_absolute: Option<&str>,
    fixed_scale: Option<(f64, f64)>,
) {
    let Some(current) = values.back().map(|sample| sample.0) else {
        frame.render_widget(
            Paragraph::new("No samples yet").block(
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
    let (scale_min, scale_max) = fixed_scale.unwrap_or_else(|| {
        let padding = ((maximum - minimum) * 0.1)
            .max(current.abs() * 0.005)
            .max(0.1);
        ((minimum - padding).max(0.0), maximum + padding)
    });
    let scale_range = (scale_max - scale_min).max(f64::EPSILON);
    let plotted = values
        .iter()
        .map(|sample| {
            ((((sample.0 - scale_min) / scale_range).clamp(0.0, 1.0)) * 1000.0).round() as u64
        })
        .collect::<Vec<_>>();
    let window = format_window(values.len());
    let current = current_absolute.map_or_else(
        || format!("{current:.1}{unit}"),
        |absolute| format!("{absolute} ({current:.1}{unit})"),
    );
    let title = format!(" {title}  now {current}  min {minimum:.1}  max {maximum:.1}  {window} ");
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
            let body = if *operation == super::Operation::Reboot {
                format!(
                    "Power cycle `{target}` on: {}\nTry graceful SSH shutdown first.\nCUT physical power even if SSH fails.\nHold outlet off for 10s, then turn it on.\nUnsaved work may be lost.\n[y/Enter] confirm  [n/Esc] cancel",
                    devices.join(", ")
                )
            } else {
                format!(
                    "{} `{target}` on:\n{}\n\n[y/Enter] confirm  [n/Esc] cancel",
                    operation.label(),
                    devices.join(", ")
                )
            };
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
