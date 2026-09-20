//! Linux systemd user timer and cron fallback artifacts.

use std::path::Path;

use super::{
    Artifact, AtTime, Backend, NativeCommand, OWNERSHIP_MARKER, PlatformPlan, ScheduleCommand,
    ScheduleId,
};

pub fn systemd(
    unit_dir: &Path,
    id: &ScheduleId,
    at: AtTime,
    executable: &Path,
    config: &Path,
    command: &ScheduleCommand,
) -> PlatformPlan {
    let base = format!("opilio-{id}");
    let timer_name = format!("{base}.timer");
    let service_name = format!("{base}.service");
    let timer = format!(
        "[Unit]\nDescription=Opilio schedule {id}\nX-Opilio-Owned={OWNERSHIP_MARKER}\n\n\
         [Timer]\nOnCalendar=*-*-* {at}:00\nPersistent=false\nUnit={service_name}\n\n\
         [Install]\nWantedBy=timers.target\n"
    );
    let service = format!(
        "[Unit]\nDescription=Opilio scheduled command {id}\nX-Opilio-Owned={OWNERSHIP_MARKER}\n\n\
         [Service]\nType=oneshot\nExecStart={}\n",
        systemd_join(&invocation(executable, config, command))
    );
    PlatformPlan {
        backend: Backend::Systemd,
        artifacts: vec![
            Artifact {
                path: unit_dir.join(&timer_name),
                contents: timer,
            },
            Artifact {
                path: unit_dir.join(&service_name),
                contents: service,
            },
        ],
        install: vec![
            native("systemctl", &["--user", "daemon-reload"]),
            native("systemctl", &["--user", "enable", "--now", &timer_name]),
        ],
        remove: vec![
            native("systemctl", &["--user", "disable", "--now", &timer_name]),
            native("systemctl", &["--user", "daemon-reload"]),
        ],
    }
}

pub fn cron_line(
    id: &ScheduleId,
    at: AtTime,
    executable: &Path,
    config: &Path,
    command: &ScheduleCommand,
) -> String {
    format!(
        "{} {} * * * {} # {OWNERSHIP_MARKER} id={id}",
        at.minute(),
        at.hour(),
        invocation(executable, config, command)
            .iter()
            .map(|argument| shell_quote(argument))
            .collect::<Vec<_>>()
            .join(" ")
    )
}

fn invocation(executable: &Path, config: &Path, command: &ScheduleCommand) -> Vec<String> {
    let mut invocation = vec![
        executable.display().to_string(),
        "--config".into(),
        config.display().to_string(),
        "--source".into(),
        "scheduled".into(),
    ];
    invocation.extend(command.args().iter().cloned());
    invocation
}

fn systemd_join(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(|argument| {
            if argument
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/._:-".contains(&byte))
            {
                argument.clone()
            } else {
                format!(
                    "\"{}\"",
                    argument
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                        .replace('%', "%%")
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn native(program: &str, args: &[&str]) -> NativeCommand {
    NativeCommand {
        program: program.into(),
        args: args.iter().map(|value| (*value).to_owned()).collect(),
    }
}
