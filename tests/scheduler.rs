use std::path::{Path, PathBuf};

use opilio::scheduler::{AtTime, Backend, ScheduleCommand, ScheduleId, platform};

fn invocation() -> ScheduleCommand {
    ScheduleCommand::new(vec![
        "alias".into(),
        "run".into(),
        "nightly shutdown".into(),
    ])
    .unwrap()
}

#[test]
fn validates_ids_times_and_closed_noninteractive_commands() {
    assert!(ScheduleId::new("nightly-01").is_ok());
    assert!(ScheduleId::new("../foreign").is_err());
    assert!("23:05".parse::<AtTime>().is_ok());
    assert!("24:00".parse::<AtTime>().is_err());

    assert!(ScheduleCommand::new(vec!["status".into(), "all".into()]).is_ok());
    assert!(ScheduleCommand::new(vec!["alias".into(), "run".into(), "nightly".into()]).is_ok());
    assert!(
        ScheduleCommand::new(vec![
            "action".into(),
            "run".into(),
            "update".into(),
            "all".into()
        ])
        .is_ok()
    );
    assert!(ScheduleCommand::new(vec!["ssh".into(), "alpha".into()]).is_err());
    assert!(ScheduleCommand::new(vec!["schedule".into(), "ls".into()]).is_err());
    assert!(ScheduleCommand::new(Vec::new()).is_err());
}

#[test]
fn systemd_artifacts_are_owned_and_quote_paths_and_arguments() {
    let plan = platform::systemd(
        Path::new("/home/a user/.config/systemd/user"),
        &ScheduleId::new("nightly").unwrap(),
        "07:30".parse().unwrap(),
        Path::new("/opt/Opilio Bin/opilio"),
        Path::new("/home/a user/flock config.yaml"),
        &invocation(),
    );

    assert_eq!(plan.backend, Backend::Systemd);
    assert_eq!(plan.artifacts.len(), 2);
    assert!(
        plan.artifacts[0]
            .contents
            .contains("X-Opilio-Owned=OPILIO_SCHEDULE_V1")
    );
    assert!(
        plan.artifacts[0]
            .contents
            .contains("OnCalendar=*-*-* 07:30:00")
    );
    assert!(plan.artifacts[1].contents.contains("--source"));
    assert!(plan.artifacts[1].contents.contains("scheduled"));
    assert!(
        plan.artifacts[1]
            .contents
            .contains("\"/opt/Opilio Bin/opilio\"")
    );
    assert!(plan.artifacts[1].contents.contains("\"nightly shutdown\""));
    assert_eq!(plan.install[1].program, PathBuf::from("systemctl"));
    assert_eq!(
        plan.install[1].args,
        ["--user", "enable", "--now", "opilio-nightly.timer"]
    );
}

#[test]
fn cron_fallback_preserves_a_safe_owned_single_line_entry() {
    let line = platform::cron_line(
        &ScheduleId::new("nightly").unwrap(),
        "07:30".parse().unwrap(),
        Path::new("/opt/Opilio Bin/opilio"),
        Path::new("/home/a user/flock.yaml"),
        &invocation(),
    );

    assert!(line.starts_with("30 7 * * * "));
    assert!(line.contains("'/opt/Opilio Bin/opilio'"));
    assert!(line.contains("'nightly shutdown'"));
    assert!(line.ends_with("# OPILIO_SCHEDULE_V1 id=nightly"));
    assert!(!line.contains('\n'));
}

#[test]
fn launchd_plist_uses_argument_elements_and_ownership_marker() {
    let plan = platform::launchd(
        Path::new("/Users/a user/Library/LaunchAgents"),
        501,
        &ScheduleId::new("nightly").unwrap(),
        "07:30".parse().unwrap(),
        Path::new("/Applications/Opi&lio/opilio"),
        Path::new("/Users/a user/flock.yaml"),
        &invocation(),
    );

    assert_eq!(plan.backend, Backend::Launchd);
    let plist = &plan.artifacts[0].contents;
    assert!(plist.contains("<key>OpilioOwnership</key>"));
    assert!(plist.contains("<string>OPILIO_SCHEDULE_V1</string>"));
    assert!(plist.contains("/Applications/Opi&amp;lio/opilio"));
    assert!(plist.contains("<integer>7</integer>"));
    assert!(plist.contains("<integer>30</integer>"));
    assert_eq!(plan.install[0].args[0], "bootstrap");
    assert_eq!(plan.install[0].args[1], "gui/501");
}

#[test]
fn windows_task_xml_and_schtasks_arguments_are_unambiguous() {
    let plan = platform::windows(
        Path::new(r"C:\Users\A User\AppData\Local\opilio\schedules"),
        &ScheduleId::new("nightly").unwrap(),
        "07:30".parse().unwrap(),
        Path::new(r"C:\Program Files\Opilio\opilio.exe"),
        Path::new(r"C:\Users\A User\flock & lab.yaml"),
        &invocation(),
    );

    assert_eq!(plan.backend, Backend::TaskScheduler);
    let xml = &plan.artifacts[0].contents;
    assert!(xml.contains("<URI>\\Opilio-nightly</URI>"));
    assert!(xml.contains("<Description>OPILIO_SCHEDULE_V1</Description>"));
    assert!(xml.contains(r"<Command>C:\Program Files\Opilio\opilio.exe</Command>"));
    assert!(xml.contains("&amp;"));
    assert!(xml.contains("&quot;nightly shutdown&quot;"));
    assert_eq!(
        plan.install[0].args[..4],
        ["/Create", "/TN", r"\Opilio-nightly", "/XML"]
    );
}
