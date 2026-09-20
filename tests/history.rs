use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use opilio::history::{
    HistoryConfig, HistoryResult, HistoryStore, HistoryWarningKind, NewHistoryRecord,
    OperationSource, Redactor,
};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

fn history_store() -> HistoryStore {
    configured_history_store(1024 * 1024, 3, 64 * 1024).0
}

fn configured_history_store(
    max_file_bytes: u64,
    max_files: usize,
    failure_tail_bytes: usize,
) -> (HistoryStore, PathBuf) {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("opilio-history-tests")
        .join(format!(
            "{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
    fs::create_dir_all(&directory).unwrap();
    let store = HistoryStore::new(HistoryConfig {
        directory: directory.clone(),
        max_file_bytes,
        max_files,
        failure_tail_bytes,
    })
    .unwrap();
    (store, directory)
}

fn failed_record(device: &str, output: &str) -> NewHistoryRecord {
    NewHistoryRecord {
        source: OperationSource::Scheduled,
        operation: "action".to_owned(),
        action: Some("deploy".to_owned()),
        requested_target: "workers".to_owned(),
        resolved_device: device.to_owned(),
        duration_ms: 7,
        result: HistoryResult::Failed,
        exit_code: Some(9),
        force: true,
        stdout: Some(output.to_owned()),
        stderr: Some(output.to_owned()),
        error: Some(format!("failed: {output}")),
    }
}

#[test]
fn append_and_query_preserve_typed_metadata_without_success_output() {
    let store = history_store();

    let record = store
        .append(NewHistoryRecord {
            source: OperationSource::Cli,
            operation: "action".to_owned(),
            action: Some("update".to_owned()),
            requested_target: "workers".to_owned(),
            resolved_device: "alpha".to_owned(),
            duration_ms: 42,
            result: HistoryResult::Succeeded,
            exit_code: Some(0),
            force: false,
            stdout: Some("sensitive successful output".to_owned()),
            stderr: Some("successful diagnostic".to_owned()),
            error: None,
        })
        .unwrap();

    let listing = store.list(None).unwrap();
    assert_eq!(listing.records, vec![record.clone()]);
    assert_eq!(record.schema_version, 1);
    assert_eq!(record.id.len(), 36);
    assert!(record.timestamp.ends_with('Z'));
    assert_eq!(record.source, OperationSource::Cli);
    assert_eq!(record.action.as_deref(), Some("update"));
    assert_eq!(record.requested_target, "workers");
    assert_eq!(record.resolved_device, "alpha");
    assert_eq!(record.duration_ms, 42);
    assert_eq!(record.result, HistoryResult::Succeeded);
    assert_eq!(record.exit_code, Some(0));
    assert!(!record.force);
    assert_eq!(record.stdout, None);
    assert_eq!(record.stderr, None);
    assert!(listing.warnings.is_empty());
}

#[test]
fn failures_are_redacted_and_tailed_on_utf8_boundaries() {
    let (_, directory) = configured_history_store(1024 * 1024, 3, 7);
    let store = HistoryStore::with_redactor(
        HistoryConfig {
            directory,
            max_file_bytes: 1024 * 1024,
            max_files: 3,
            failure_tail_bytes: 7,
        },
        Redactor::new(["token-秘密".to_owned()]),
    )
    .unwrap();

    let record = store
        .append(failed_record("alpha", "prefix token-秘密 trailing-🙂🙂"))
        .unwrap();

    assert_eq!(record.stdout.as_deref(), Some("🙂"));
    assert_eq!(record.stderr.as_deref(), Some("🙂"));
    assert_eq!(record.error.as_deref(), Some("🙂"));
    let serialized = serde_json::to_string(&store.list(None).unwrap()).unwrap();
    assert!(!serialized.contains("token-秘密"));
}

#[test]
fn redaction_covers_raw_and_url_encoded_secret_values() {
    let (_, directory) = configured_history_store(1024 * 1024, 3, 256);
    let store = HistoryStore::with_redactor(
        HistoryConfig {
            directory,
            max_file_bytes: 1024 * 1024,
            max_files: 3,
            failure_tail_bytes: 256,
        },
        Redactor::new(["token-秘密".to_owned()]),
    )
    .unwrap();

    store
        .append(failed_record(
            "alpha",
            "raw=token-秘密 encoded=token-%E7%A7%98%E5%AF%86",
        ))
        .unwrap();

    let json = serde_json::to_string(&store.list(None).unwrap()).unwrap();
    assert!(!json.contains("token-秘密"));
    assert!(!json.contains("token-%E7%A7%98%E5%AF%86"));
    assert_eq!(json.matches("[REDACTED]").count(), 6);
}

#[test]
fn rotation_retains_configured_files_and_queries_across_them() {
    let (store, directory) = configured_history_store(1, 3, 64);
    let first = store.append(failed_record("alpha", "one")).unwrap();
    let second = store.append(failed_record("beta", "two")).unwrap();
    let third = store.append(failed_record("gamma", "three")).unwrap();
    let fourth = store.append(failed_record("delta", "four")).unwrap();

    assert!(directory.join("history.jsonl").exists());
    assert!(directory.join("history.1.jsonl").exists());
    assert!(directory.join("history.2.jsonl").exists());
    assert!(!directory.join("history.3.jsonl").exists());
    let listing = store.list(None).unwrap();
    assert_eq!(listing.records.len(), 3);
    assert!(!listing.records.contains(&first));
    assert!(listing.records.contains(&second));
    assert!(listing.records.contains(&third));
    assert!(listing.records.contains(&fourth));
}

#[test]
fn filtering_and_show_use_requested_or_resolved_target_and_stable_id() {
    let store = history_store();
    let alpha = store.append(failed_record("alpha", "failure")).unwrap();
    store.append(failed_record("beta", "failure")).unwrap();

    assert_eq!(
        store.list(Some("alpha")).unwrap().records,
        vec![alpha.clone()]
    );
    assert_eq!(store.list(Some("workers")).unwrap().records.len(), 2);
    assert_eq!(store.show(&alpha.id).unwrap().0, alpha);
    assert!(
        store
            .show("missing")
            .unwrap_err()
            .to_string()
            .contains("not found")
    );
}

#[test]
fn corrupt_and_truncated_records_are_reported_without_fake_results() {
    let (store, directory) = configured_history_store(1024 * 1024, 3, 64);
    let valid = store.append(failed_record("alpha", "failure")).unwrap();
    fs::write(
        directory.join("history.jsonl"),
        format!(
            "{}\n{{not-json}}\n{{\"schema_version\":1",
            serde_json::to_string(&valid).unwrap()
        ),
    )
    .unwrap();

    let listing = store.list(None).unwrap();

    assert_eq!(listing.records, vec![valid]);
    assert_eq!(listing.warnings.len(), 2);
    assert_eq!(
        listing.warnings[0].kind,
        HistoryWarningKind::MalformedRecord
    );
    assert_eq!(
        listing.warnings[1].kind,
        HistoryWarningKind::TruncatedRecord
    );

    let recovered = store
        .append(failed_record("beta", "later failure"))
        .unwrap();
    let listing = store.list(None).unwrap();
    assert!(listing.records.contains(&recovered));
    assert_eq!(listing.warnings.len(), 2);
}

#[test]
fn concurrent_appends_do_not_lose_or_corrupt_records() {
    let store = history_store();
    std::thread::scope(|scope| {
        for index in 0..16 {
            let store = store.clone();
            scope.spawn(move || {
                store
                    .append(failed_record(&format!("device-{index}"), "failure"))
                    .unwrap();
            });
        }
    });

    let listing = store.list(None).unwrap();
    assert_eq!(listing.records.len(), 16);
    assert!(listing.warnings.is_empty());
}
