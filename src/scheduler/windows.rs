//! Windows Task Scheduler XML artifacts.

use std::path::Path;

use super::{
    Artifact, AtTime, Backend, NativeCommand, OWNERSHIP_MARKER, PlatformPlan, ScheduleCommand,
    ScheduleId,
};

pub fn task_scheduler(
    state_dir: &Path,
    id: &ScheduleId,
    at: AtTime,
    executable: &Path,
    config: &Path,
    command: &ScheduleCommand,
) -> PlatformPlan {
    let task_name = format!(r"\Opilio-{id}");
    let path = state_dir.join(format!("{id}.task.xml"));
    let mut arguments = vec![
        "--config".to_owned(),
        config.display().to_string(),
        "--source".into(),
        "scheduled".into(),
    ];
    arguments.extend(command.args().iter().cloned());
    let arguments = windows_join(&arguments);
    let contents = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n\
         <RegistrationInfo><URI>{}</URI><Description>{OWNERSHIP_MARKER}</Description></RegistrationInfo>\n\
         <Triggers><CalendarTrigger><StartBoundary>2000-01-01T{}:00</StartBoundary>\
         <Enabled>true</Enabled><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>\
         </CalendarTrigger></Triggers>\n\
         <Principals><Principal id=\"Author\"><LogonType>InteractiveToken</LogonType>\
         <RunLevel>LeastPrivilege</RunLevel></Principal></Principals>\n\
         <Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\
         <StartWhenAvailable>false</StartWhenAvailable><Enabled>true</Enabled></Settings>\n\
         <Actions Context=\"Author\"><Exec><Command>{}</Command><Arguments>{}</Arguments>\
         </Exec></Actions></Task>\n",
        xml(&task_name),
        at,
        xml(&executable.display().to_string()),
        xml(&arguments)
    );
    PlatformPlan {
        backend: Backend::TaskScheduler,
        artifacts: vec![Artifact {
            path: path.clone(),
            contents,
        }],
        install: vec![NativeCommand {
            program: "schtasks.exe".into(),
            args: vec![
                "/Create".into(),
                "/TN".into(),
                task_name.clone(),
                "/XML".into(),
                path.display().to_string(),
            ],
        }],
        remove: vec![NativeCommand {
            program: "schtasks.exe".into(),
            args: vec!["/Delete".into(), "/TN".into(), task_name, "/F".into()],
        }],
    }
}

fn windows_join(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(|argument| {
            if argument.is_empty()
                || argument
                    .bytes()
                    .any(|byte| byte.is_ascii_whitespace() || byte == b'"')
            {
                let mut quoted = String::from("\"");
                let mut slashes = 0;
                for character in argument.chars() {
                    if character == '\\' {
                        slashes += 1;
                    } else if character == '"' {
                        quoted.push_str(&"\\".repeat(slashes * 2 + 1));
                        quoted.push('"');
                        slashes = 0;
                    } else {
                        quoted.push_str(&"\\".repeat(slashes));
                        slashes = 0;
                        quoted.push(character);
                    }
                }
                quoted.push_str(&"\\".repeat(slashes * 2));
                quoted.push('"');
                quoted
            } else {
                argument.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
