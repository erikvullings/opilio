//! macOS launchd artifacts.

use std::path::Path;

use super::{
    Artifact, AtTime, Backend, NativeCommand, OWNERSHIP_MARKER, PlatformPlan, ScheduleCommand,
    ScheduleId,
};

pub fn launchd(
    launch_agents: &Path,
    uid: u32,
    id: &ScheduleId,
    at: AtTime,
    executable: &Path,
    config: &Path,
    command: &ScheduleCommand,
) -> PlatformPlan {
    let label = format!("io.github.opilio.schedule.{id}");
    let path = launch_agents.join(format!("{label}.plist"));
    let mut arguments = vec![
        executable.display().to_string(),
        "--config".into(),
        config.display().to_string(),
        "--source".into(),
        "scheduled".into(),
    ];
    arguments.extend(command.args().iter().cloned());
    let argument_xml = arguments
        .iter()
        .map(|argument| format!("    <string>{}</string>", xml(argument)))
        .collect::<Vec<_>>()
        .join("\n");
    let contents = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         <key>Label</key><string>{label}</string>\n\
         <key>OpilioOwnership</key><string>{OWNERSHIP_MARKER}</string>\n\
         <key>ProgramArguments</key>\n<array>\n{argument_xml}\n</array>\n\
         <key>StartCalendarInterval</key>\n<dict>\n\
         <key>Hour</key><integer>{}</integer>\n\
         <key>Minute</key><integer>{}</integer>\n\
         </dict>\n<key>RunAtLoad</key><false/>\n</dict>\n</plist>\n",
        at.hour(),
        at.minute()
    );
    let domain = format!("gui/{uid}");
    PlatformPlan {
        backend: Backend::Launchd,
        artifacts: vec![Artifact {
            path: path.clone(),
            contents,
        }],
        install: vec![NativeCommand {
            program: "launchctl".into(),
            args: vec![
                "bootstrap".into(),
                domain.clone(),
                path.display().to_string(),
            ],
        }],
        remove: vec![NativeCommand {
            program: "launchctl".into(),
            args: vec!["bootout".into(), format!("{domain}/{label}")],
        }],
    }
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
