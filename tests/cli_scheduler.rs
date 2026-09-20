use std::{
    fs,
    path::PathBuf,
    sync::Mutex,
    sync::atomic::{AtomicUsize, Ordering},
};

use clap::Parser;
use opilio::{
    app,
    cli::Cli,
    scheduler::{
        AddSchedule, Backend, ScheduleEntry, ScheduleError, ScheduleId, ScheduleListing, Scheduler,
    },
    status::ExitStatus,
};

static NEXT_CASE: AtomicUsize = AtomicUsize::new(0);

fn config_path() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("opilio-scheduler-tests");
    fs::create_dir_all(&root).unwrap();
    let path = root.join(format!(
        "{}-{}.yaml",
        std::process::id(),
        NEXT_CASE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, "devices:\n  alpha:\n    ssh: alpha\n").unwrap();
    path
}

#[derive(Default)]
struct FakeScheduler {
    jobs: Mutex<Vec<ScheduleEntry>>,
}

impl Scheduler for FakeScheduler {
    fn list(&self) -> Result<ScheduleListing, ScheduleError> {
        Ok(ScheduleListing {
            schema_version: 1,
            jobs: self.jobs.lock().unwrap().clone(),
            warnings: Vec::new(),
        })
    }

    fn add(&self, request: AddSchedule) -> Result<ScheduleEntry, ScheduleError> {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.iter().any(|job| job.id == request.id.as_str()) {
            return Err(ScheduleError::AlreadyExists(request.id.to_string()));
        }
        let job = ScheduleEntry {
            id: request.id.to_string(),
            at: request.at.to_string(),
            command: request.command.args().to_vec(),
            backend: Backend::Launchd,
        };
        jobs.push(job.clone());
        Ok(job)
    }

    fn remove(&self, id: &ScheduleId) -> Result<ScheduleEntry, ScheduleError> {
        let mut jobs = self.jobs.lock().unwrap();
        let index = jobs
            .iter()
            .position(|job| job.id == id.as_str())
            .ok_or_else(|| ScheduleError::NotOwned(id.to_string()))?;
        Ok(jobs.remove(index))
    }
}

#[test]
fn add_list_remove_round_trip_has_stable_human_and_json_output() {
    let config = config_path();
    let scheduler = FakeScheduler::default();
    let mut added = Vec::new();
    let status = app::execute_with_scheduler(
        Cli::try_parse_from([
            "opilio",
            "--config",
            config.to_str().unwrap(),
            "schedule",
            "add",
            "nightly",
            "--at",
            "22:15",
            "--json",
            "--",
            "alias",
            "run",
            "shutdown-fleet",
        ])
        .unwrap(),
        &mut added,
        &scheduler,
    )
    .unwrap();
    assert_eq!(status, ExitStatus::Success);
    let json: serde_json::Value = serde_json::from_slice(&added).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["job"]["id"], "nightly");
    assert_eq!(json["job"]["at"], "22:15");
    assert_eq!(
        json["job"]["command"],
        serde_json::json!(["alias", "run", "shutdown-fleet"])
    );

    let mut listed = Vec::new();
    app::execute_with_scheduler(
        Cli::try_parse_from([
            "opilio",
            "--config",
            config.to_str().unwrap(),
            "schedule",
            "ls",
            "--json",
        ])
        .unwrap(),
        &mut listed,
        &scheduler,
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&listed).unwrap();
    assert_eq!(json["jobs"].as_array().unwrap().len(), 1);

    let mut removed = Vec::new();
    app::execute_with_scheduler(
        Cli::try_parse_from([
            "opilio",
            "--config",
            config.to_str().unwrap(),
            "schedule",
            "rm",
            "nightly",
        ])
        .unwrap(),
        &mut removed,
        &scheduler,
    )
    .unwrap();
    assert_eq!(String::from_utf8(removed).unwrap(), "removed nightly\n");
    assert!(scheduler.list().unwrap().jobs.is_empty());
}

#[test]
fn removal_of_a_non_owned_job_is_denied() {
    let config = config_path();
    let error = app::execute_with_scheduler(
        Cli::try_parse_from([
            "opilio",
            "--config",
            config.to_str().unwrap(),
            "schedule",
            "rm",
            "foreign",
        ])
        .unwrap(),
        &mut Vec::new(),
        &FakeScheduler::default(),
    )
    .unwrap_err();

    assert!(error.to_string().contains("not owned by Opilio"));
    assert_eq!(error.exit_code(), 2);
}

#[test]
fn listing_warnings_produce_partial_success_without_hiding_owned_jobs() {
    struct WarningScheduler;
    impl Scheduler for WarningScheduler {
        fn list(&self) -> Result<ScheduleListing, ScheduleError> {
            Ok(ScheduleListing {
                schema_version: 1,
                jobs: vec![ScheduleEntry {
                    id: "owned".into(),
                    at: "01:00".into(),
                    command: vec!["status".into(), "all".into()],
                    backend: Backend::Cron,
                }],
                warnings: vec!["ignored malformed Opilio metadata bad.json".into()],
            })
        }
        fn add(&self, _: AddSchedule) -> Result<ScheduleEntry, ScheduleError> {
            unreachable!()
        }
        fn remove(&self, _: &ScheduleId) -> Result<ScheduleEntry, ScheduleError> {
            unreachable!()
        }
    }

    let status = app::execute_with_scheduler(
        Cli::try_parse_from([
            "opilio",
            "--config",
            config_path().to_str().unwrap(),
            "schedule",
            "ls",
            "--quiet",
        ])
        .unwrap(),
        &mut Vec::new(),
        &WarningScheduler,
    )
    .unwrap();
    assert_eq!(status, ExitStatus::PartialSuccess);
}
