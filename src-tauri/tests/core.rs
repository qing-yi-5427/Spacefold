use spacefold_lib::{
    export, file_ops,
    model::*,
    platform,
    repository::{insert_node, Repository},
    service::{JobRuntime, Service},
};
use std::{
    fs,
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

fn fixture() -> (tempfile::TempDir, Arc<Service>, Scan) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("source");
    fs::create_dir_all(root.join("中文🧭/nested")).unwrap();
    fs::create_dir_all(root.join("empty")).unwrap();
    fs::write(root.join("root.bin"), [1u8; 17]).unwrap();
    fs::write(root.join("中文🧭/nested/a.bin"), [2u8; 31]).unwrap();
    fs::write(root.join("中文🧭/=SUM(1,2).txt"), [3u8; 11]).unwrap();
    fs::write(root.join("zero.bin"), []).unwrap();
    fs::hard_link(root.join("root.bin"), root.join("hard.bin")).unwrap();
    let service = Service::new(&dir.path().join("db")).unwrap();
    let s = service
        .start_scan(platform::absolute(&root).unwrap(), None)
        .unwrap();
    let scan = wait_scan(&service, &s.scan_id);
    assert_eq!(scan.status, "completed");
    assert_eq!(scan.logical_total_bytes, "76");
    (dir, service, scan)
}
fn wait_scan(service: &Service, id: &str) -> Scan {
    let start = Instant::now();
    loop {
        let s = service.repo.scan(id).unwrap();
        if !matches!(s.status.as_str(), "queued" | "running" | "cancelling") {
            return s;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "scan timeout");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn wait_job(service: &Service, id: &str) -> JobEvent {
    let start = Instant::now();
    loop {
        let j = service.job(id).unwrap();
        if j.terminal() {
            return j.snapshot();
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "job timeout {:?}",
            j.snapshot()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn find(service: &Service, scan: &Scan, name: &str) -> Node {
    service
        .repo
        .page(
            &PageRequest {
                scan_id: scan.scan_id.clone(),
                revision: scan.revision,
                parent_id: None,
                sort: None,
                filter: Some(Filter {
                    keyword: Some(name.into()),
                    ..Default::default()
                }),
                cursor: None,
                page_size: Some(200),
            },
            true,
        )
        .unwrap()
        .nodes
        .into_iter()
        .find(|n| n.name == name)
        .unwrap()
}
fn request(scan: &Scan, format: &str) -> ExportRequest {
    ExportRequest {
        scan_id: scan.scan_id.clone(),
        revision: scan.revision,
        scope: "all".into(),
        scope_id: "1".into(),
        selected_ids: vec![],
        recursive: true,
        fields: export::FIELDS.iter().map(|s| s.to_string()).collect(),
        format: format.into(),
        target_path: None,
        html_row_budget: None,
    }
}
#[test]
fn real_scan_hardlinks_empty_root_children_and_selection() {
    let (_dir, service, mut scan) = fixture();
    let root = service
        .repo
        .node(&scan.scan_id, "1", Some(scan.revision))
        .unwrap();
    assert_eq!(root.node.file_count, "5");
    assert_eq!(root.node.directory_count, "3");
    assert!(
        rusqlite::version_number() >= 3_051_003,
        "SQLite {} is missing the WAL-reset fix",
        rusqlite::version()
    );
    for (search, kind, total) in [
        (false, "file", "3"),
        (false, "directory", "2"),
        (true, "file", "5"),
        (true, "directory", "3"),
    ] {
        let page = service
            .repo
            .page(
                &PageRequest {
                    scan_id: scan.scan_id.clone(),
                    revision: scan.revision,
                    parent_id: Some("1".into()),
                    sort: None,
                    cursor: None,
                    page_size: Some(200),
                    filter: Some(Filter {
                        kind: Some(kind.into()),
                        ..Default::default()
                    }),
                },
                search,
            )
            .unwrap();
        assert_eq!(page.total_count, total);
        assert_eq!(page.nodes.len(), total.parse::<usize>().unwrap());
    }
    let folder = find(&service, &scan, "中文🧭");
    let child = find(&service, &scan, "a.bin");
    let summary = service
        .repo
        .selection(
            &scan.scan_id,
            &[folder.node_id.clone(), child.node_id.clone()],
            scan.revision,
        )
        .unwrap();
    assert_eq!(summary.count, "1");
    assert_eq!(summary.logical_total_bytes, "42");
    assert_eq!(summary.covered_ids, vec![child.node_id]);
    assert_eq!(find(&service, &scan, "empty").logical_total_bytes, "0");
    let hard = find(&service, &scan, "hard.bin");
    let hard_was_unknown = hard.allocated_bytes.is_none();
    // NTFS bulk enumeration already measures allocation; the supported legacy
    // fallback retains unknown values until the same explicit detail operation.
    assert_eq!(
        root.node.allocated_unknown_count,
        if hard_was_unknown { "5" } else { "0" }
    );
    let (measured, revision) = service
        .measure_node(&scan.scan_id, &hard.node_id, scan.revision)
        .unwrap();
    assert!(measured.allocated_bytes.is_some());
    scan.revision = revision;
    assert_eq!(
        service
            .repo
            .node(&scan.scan_id, "1", Some(revision))
            .unwrap()
            .node
            .allocated_unknown_count,
        if hard_was_unknown { "4" } else { "0" }
    );
    assert!(
        measured
            .link_count
            .as_deref()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            >= 2
    );
    let page = service
        .repo
        .page(
            &PageRequest {
                scan_id: scan.scan_id.clone(),
                parent_id: Some("1".into()),
                revision: scan.revision,
                sort: None,
                filter: None,
                cursor: None,
                page_size: Some(2),
            },
            false,
        )
        .unwrap();
    assert_eq!(page.nodes.len(), 2);
    assert!(page.next_cursor.is_some());
    assert!(
        service
            .repo
            .node(&scan.scan_id, "1", Some(scan.revision - 1))
            .unwrap_err()
            .code
            == "REVISION_CHANGED"
    );
}
#[test]
fn four_reports_read_back_and_shared_range() {
    let (dir, service, scan) = fixture();
    for format in ["csv", "json", "html", "xlsx"] {
        let r = request(&scan, format);
        let target = dir.path().join(format!("report.{format}"));
        let job = JobRuntime::new(
            JobEvent::new("test", &scan.scan_id, "export", scan.revision),
            None,
        );
        let preview = export::preview(&service, &r).unwrap();
        let count = export::write_report(&service, &r, &target, &job).unwrap();
        assert_eq!(count.to_string(), preview.record_count);
        match format {
            "json" => {
                let v: serde_json::Value =
                    serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
                assert_eq!(v["rows"].as_array().unwrap().len(), count as usize);
                assert_eq!(v["metadata"]["totalLogicalBytes"], "76");
                assert!(v["rows"][0]["pathEncoding"].is_string());
            }
            "csv" => {
                let b = fs::read(&target).unwrap();
                assert_eq!(&b[..3], &[0xef, 0xbb, 0xbf]);
                let mut reader = csv::Reader::from_reader(&b[3..]);
                let records = reader
                    .records()
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .unwrap();
                assert_eq!(records.len(), count as usize + 1);
                assert_eq!(&records[0][0], "metadata");
                let meta: serde_json::Value = serde_json::from_str(&records[0][1]).unwrap();
                assert_eq!(meta["recordCount"], count.to_string());
                assert!(records
                    .iter()
                    .any(|r| r.iter().any(|v| v.starts_with("'=SUM"))));
            }
            "html" => {
                let html = fs::read_to_string(&target).unwrap();
                assert!(html.contains("<!doctype html>"));
                assert!(html.contains("Spacefold"));
                assert_eq!(html.matches("<tr>").count(), count as usize + 1);
            }
            "xlsx" => {
                use std::io::Read;
                let mut zip = zip::ZipArchive::new(fs::File::open(&target).unwrap()).unwrap();
                let mut xml = String::new();
                zip.by_name("xl/worksheets/sheet2.xml")
                    .unwrap()
                    .read_to_string(&mut xml)
                    .unwrap();
                assert_eq!(xml.matches("<row ").count(), count as usize + 1);
                assert!(xml.contains("autoFilter"));
                assert!(xml.contains("frozen"));
                assert!(!xml.contains("<f>"));
            }
            _ => unreachable!(),
        }
    }
    let folder = find(&service, &scan, "中文🧭");
    let child = find(&service, &scan, "a.bin");
    let mut r = request(&scan, "json");
    r.scope = "selected".into();
    r.selected_ids = vec![folder.node_id.clone(), child.node_id.clone()];
    let preview = export::preview(&service, &r).unwrap();
    assert_eq!(preview.logical_total_bytes, "42");
    assert_eq!(preview.record_count, "4");
    r.recursive = false;
    assert_eq!(export::preview(&service, &r).unwrap().record_count, "1");
}
#[test]
fn export_cancel_and_limits_preserve_prior_target() {
    let (dir, service, scan) = fixture();
    let target = dir.path().join("original.html");
    fs::write(&target, "ORIGINAL").unwrap();
    let mut r = request(&scan, "html");
    r.html_row_budget = Some(1);
    let job = JobRuntime::new(
        JobEvent::new("test", &scan.scan_id, "export", scan.revision),
        None,
    );
    assert_eq!(
        export::write_report(&service, &r, &target, &job)
            .unwrap_err()
            .code,
        "HTML_ROW_BUDGET"
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "ORIGINAL");
    job.cancel.store(true, Ordering::Release);
    assert_eq!(
        export::write_report(&service, &request(&scan, "json"), &target, &job)
            .unwrap_err()
            .code,
        "CANCELLED"
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "ORIGINAL");
    assert_eq!(
        fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp"))
            .count(),
        0
    );
}
#[test]
fn integer_precision_sql_range_and_settings_recovery() {
    let (dir, service, _scan) = fixture();
    let root = dir.path().join("synthetic");
    fs::create_dir(&root).unwrap();
    let id = "precise";
    service.repo.insert_scan(id, &root, true).unwrap();
    let mut c = service.repo.connect().unwrap();
    let tx = c.transaction().unwrap();
    let m = platform::metadata(&root).unwrap();
    insert_node(&tx, id, 1, None, &root, 0, &m, None).unwrap();
    let mut file = m.clone();
    file.kind = "file".into();
    file.recurse = false;
    file.logical = 9_007_199_254_740_993;
    file.allocated = Some(0);
    insert_node(&tx, id, 2, Some(1), &root.join("large"), 1, &file, None).unwrap();
    tx.commit().unwrap();
    let scan = service.repo.finish(id, "completed").unwrap();
    assert_eq!(scan.logical_total_bytes, "9007199254740993");
    let target = dir.path().join("precise.json");
    export::write_report(
        &service,
        &request(&scan, "json"),
        &target,
        &JobRuntime::new(JobEvent::new("t", id, "export", scan.revision), None),
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&fs::read(target).unwrap()).unwrap();
    assert_eq!(value["rows"][0]["logical_bytes"], "9007199254740993");
    assert_eq!(integer(u64::MAX).unwrap_err().code, "SIZE_OUT_OF_RANGE");
    let settings = Settings {
        accent: "rose".into(),
        ..Default::default()
    };
    service.save_settings(settings).unwrap();
    assert_eq!(service.read_settings().accent, "rose");
    fs::write(service.repo.data_dir.join("settings.json"), "broken").unwrap();
    assert_eq!(service.read_settings().accent, "purple");
}
#[test]
fn recovery_marks_running_scan_interrupted() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::new(temp.path()).unwrap();
    repo.insert_scan("interrupted", temp.path(), false).unwrap();
    let c = repo.connect().unwrap();
    c.execute(
        "UPDATE scans SET status='running' WHERE id='interrupted'",
        [],
    )
    .unwrap();
    drop(c);
    drop(repo);
    let reopened = Repository::new(temp.path()).unwrap();
    let s = reopened.scan("interrupted").unwrap();
    assert_eq!(s.status, "interrupted");
    assert!(s.incomplete && s.stale);
}
fn operation_request(
    scan: &Scan,
    node: &Node,
    kind: &str,
    dest: Option<&Path>,
    name: Option<&str>,
    policy: &str,
) -> OperationRequest {
    OperationRequest {
        scan_id: scan.scan_id.clone(),
        node_ids: vec![node.node_id.clone()],
        expected_revision: scan.revision,
        kind: kind.into(),
        destination_ref: None,
        destination_path: dest.map(platform::display),
        new_name: name.map(str::to_owned),
        conflict_policy: Some(policy.into()),
    }
}
#[cfg(windows)]
#[test]
fn real_copy_rename_move_auto_refresh_and_conflicts() {
    let (dir, service, scan) = fixture();
    let dest = dir.path().join("destination");
    fs::create_dir(&dest).unwrap();
    let node = find(&service, &scan, "root.bin");
    let plan = file_ops::plan(
        &service,
        operation_request(&scan, &node, "copy", Some(&dest), None, "skip"),
    )
    .unwrap();
    assert!(plan.items[0].error.is_none(), "{:?}", plan);
    let job = file_ops::execute(&service, &plan.plan_id, None).unwrap();
    let done = wait_job(&service, &job.job_id);
    assert_eq!(
        done.results.as_ref().unwrap()[0].status,
        "success",
        "{done:?}"
    );
    assert_eq!(fs::read(dest.join("root.bin")).unwrap(), vec![1u8; 17]);
    let scan = service.repo.scan(&scan.scan_id).unwrap();
    let plan = file_ops::plan(
        &service,
        operation_request(&scan, &node, "copy", Some(&dest), None, "skip"),
    )
    .unwrap();
    let job = file_ops::execute(&service, &plan.plan_id, None).unwrap();
    assert_eq!(
        wait_job(&service, &job.job_id).results.unwrap()[0].status,
        "skipped"
    );
    let plan = file_ops::plan(
        &service,
        operation_request(&scan, &node, "rename", None, Some("renamed.bin"), "skip"),
    )
    .unwrap();
    let job = file_ops::execute(&service, &plan.plan_id, None).unwrap();
    let done = wait_job(&service, &job.job_id);
    assert_eq!(
        done.results.as_ref().unwrap()[0].status,
        "success",
        "{done:?}"
    );
    assert!(dir.path().join("source/renamed.bin").exists());
    let scan = service.repo.scan(&scan.scan_id).unwrap();
    assert!(!scan.stale);
    assert_eq!(scan.logical_total_bytes, "76");
    let rename = find(&service, &scan, "renamed.bin");
    let plan = file_ops::plan(
        &service,
        operation_request(&scan, &rename, "move", Some(&dest), None, "skip"),
    )
    .unwrap();
    let job = file_ops::execute(&service, &plan.plan_id, None).unwrap();
    let done = wait_job(&service, &job.job_id);
    assert_eq!(
        done.results.as_ref().unwrap()[0].status,
        "success",
        "{done:?}"
    );
    let scan = service.repo.scan(&scan.scan_id).unwrap();
    assert_eq!(scan.logical_total_bytes, "59");
    assert!(!scan.stale);
}
#[cfg(windows)]
#[test]
fn source_replacement_and_late_destination_are_rejected() {
    let (dir, service, scan) = fixture();
    let dest = dir.path().join("destination");
    fs::create_dir(&dest).unwrap();
    let node = find(&service, &scan, "zero.bin");
    let plan = file_ops::plan(
        &service,
        operation_request(&scan, &node, "move", Some(&dest), None, "skip"),
    )
    .unwrap();
    fs::rename(
        dir.path().join("source/zero.bin"),
        dir.path().join("source/old.bin"),
    )
    .unwrap();
    fs::write(dir.path().join("source/zero.bin"), "replacement").unwrap();
    let job = file_ops::execute(&service, &plan.plan_id, None).unwrap();
    let done = wait_job(&service, &job.job_id);
    assert_eq!(
        done.results.unwrap()[0].error.as_ref().unwrap().code,
        "SOURCE_CHANGED"
    );
    assert!(dir.path().join("source/zero.bin").exists());
    let node = find(&service, &scan, "root.bin");
    let plan = file_ops::plan(
        &service,
        operation_request(&scan, &node, "copy", Some(&dest), None, "skip"),
    )
    .unwrap();
    fs::write(dest.join("root.bin"), "foreign").unwrap();
    let job = file_ops::execute(&service, &plan.plan_id, None).unwrap();
    let done = wait_job(&service, &job.job_id);
    assert_eq!(
        done.results.unwrap()[0].error.as_ref().unwrap().code,
        "DESTINATION_CONFLICT"
    );
    assert_eq!(
        fs::read_to_string(dest.join("root.bin")).unwrap(),
        "foreign"
    );
}
#[test]
fn scan_cancellation_keeps_partial_and_terminal_waits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("source");
    fs::create_dir(&root).unwrap();
    for i in 0..2000 {
        fs::write(root.join(format!("f{i}")), [1u8; 1]).unwrap();
    }
    let service = Service::new(&dir.path().join("db")).unwrap();
    let scan = service
        .start_scan(platform::absolute(&root).unwrap(), None)
        .unwrap();
    assert!(service.repo.node(&scan.scan_id, "1", Some(1)).is_ok());
    let cancel_started = Instant::now();
    service.cancel_job(&scan.scan_id).unwrap();
    let done = wait_scan(&service, &scan.scan_id);
    assert_eq!(done.status, "cancelled");
    assert!(done.incomplete);
    // The durable status commits before the channel event. Observe the final
    // event rather than imposing atomicity across two independent interfaces.
    let final_event = wait_job(&service, &scan.scan_id);
    assert_eq!(final_event.status, "cancelled");
    assert_eq!(final_event.revision, done.revision);
    assert_eq!(final_event.discovered_count, done.discovered_count);
    assert_eq!(final_event.logical_total_bytes, done.logical_total_bytes);
    assert!(cancel_started.elapsed() < Duration::from_secs(1));
}

#[test]
fn completed_event_observes_closed_scanner_and_complete_main_database() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("source");
    fs::create_dir(&root).unwrap();
    for i in 0..512 {
        fs::write(root.join(format!("f{i}")), b"content").unwrap();
    }
    let service = Service::new(&dir.path().join("db")).unwrap();
    let c = service.repo.connect().unwrap();
    let filename: String = c
        .query_row("PRAGMA database_list", [], |r| r.get(2))
        .unwrap();
    drop(c);
    let encoded = filename
        .replace('\\', "/")
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"/:._-".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect::<String>();
    let uri = format!("file:{encoded}?immutable=1");
    let (done_tx, done_rx) = crossbeam_channel::bounded(1);
    let scan = service
        .start_scan(
            platform::absolute(&root).unwrap(),
            Some(Arc::new(move |event| {
                if event.status != "completed" {
                    return;
                }
                // immutable deliberately ignores WAL and cannot checkpoint it, so this
                // callback catches a terminal event sent before scanner connection close.
                let observed = (|| -> rusqlite::Result<(String, i64, i64, i64)> {
                    let c = rusqlite::Connection::open_with_flags(
                        &uri,
                        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                            | rusqlite::OpenFlags::SQLITE_OPEN_URI,
                    )?;
                    let (status, revision): (String, i64) = c.query_row(
                        "SELECT status,revision FROM scans WHERE id=?1",
                        [&event.scan_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )?;
                    let count = c.query_row(
                        "SELECT COUNT(*) FROM nodes WHERE scan_id=?1",
                        [&event.scan_id],
                        |r| r.get(0),
                    )?;
                    let logical = c.query_row(
                        "SELECT logical_total FROM nodes WHERE scan_id=?1 AND id=1",
                        [&event.scan_id],
                        |r| r.get(0),
                    )?;
                    Ok((status, revision, count, logical))
                })()
                .map_err(|e| e.to_string());
                done_tx.send((event.revision, observed)).unwrap();
            })),
        )
        .unwrap();
    let final_event = wait_job(&service, &scan.scan_id);
    assert_eq!(final_event.status, "completed");
    let (revision, observed) = done_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let (status, stored_revision, count, logical) = observed.unwrap();
    assert_eq!(status, "completed");
    assert_eq!(stored_revision, i64::from(revision));
    assert_eq!(count, 513);
    assert_eq!(logical, 3584);
}

#[cfg(windows)]
#[test]
fn real_recycle_returns_item_result_and_refreshes_scan() {
    let (dir, service, scan) = fixture();
    let node = find(&service, &scan, "zero.bin");
    let plan = file_ops::plan(
        &service,
        operation_request(&scan, &node, "trash", None, None, "skip"),
    )
    .unwrap();
    assert!(plan.items[0].error.is_none(), "{plan:?}");
    let job = file_ops::execute(&service, &plan.plan_id, None).unwrap();
    let event = wait_job(&service, &job.job_id);
    assert_eq!(
        event.results.as_ref().unwrap()[0].status,
        "success",
        "{event:?}"
    );
    assert!(!dir.path().join("source/zero.bin").exists());
    let refreshed = service.repo.scan(&scan.scan_id).unwrap();
    assert_eq!(refreshed.discovered_count, "8");
    assert!(!refreshed.stale);
}

#[cfg(windows)]
#[test]
fn junction_is_not_followed_and_changed_parent_rejects_operation() {
    let (dir, service, scan) = fixture();
    let parent = dir.path().join("source/中文🧭/nested");
    let target = dir.path().join("moved-original");
    let destination = dir.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let node = find(&service, &scan, "a.bin");
    let plan = file_ops::plan(
        &service,
        operation_request(&scan, &node, "move", Some(&destination), None, "skip"),
    )
    .unwrap();
    fs::rename(&parent, &target).unwrap();
    let result = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(parent.to_string_lossy().replace('/', "\\"))
        .arg(target.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stdout {} stderr {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let job = file_ops::execute(&service, &plan.plan_id, None).unwrap();
    let event = wait_job(&service, &job.job_id);
    assert_eq!(
        event.results.as_ref().unwrap()[0].status,
        "failed",
        "{event:?}"
    );
    assert!(
        matches!(
            event.results.as_ref().unwrap()[0]
                .error
                .as_ref()
                .unwrap()
                .code
                .as_str(),
            "SOURCE_CHANGED" | "UNSAFE_PARENT"
        ),
        "{event:?}"
    );
    assert_eq!(fs::read(target.join("a.bin")).unwrap(), vec![2; 31]);
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
    let latest = service
        .start_scan(
            platform::absolute(&dir.path().join("source")).unwrap(),
            None,
        )
        .unwrap();
    let latest = wait_scan(&service, &latest.scan_id);
    assert_eq!(latest.logical_total_bytes, "45");
    let link = find(&service, &latest, "nested");
    assert_eq!(link.kind, "junction");
    assert_eq!(link.children_count, "0");
    fs::remove_dir(&parent).unwrap();
}

#[test]
fn app_work_directory_excluded_and_export_cannot_overwrite_index() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("visible.txt"), "123").unwrap();
    let service = Service::new(&root.join("app-data")).unwrap();
    let scan = service
        .start_scan(platform::absolute(&root).unwrap(), None)
        .unwrap();
    let scan = wait_scan(&service, &scan.scan_id);
    assert_eq!(scan.discovered_count, "2");
    assert_eq!(scan.logical_total_bytes, "3");
    let job = JobRuntime::new(
        JobEvent::new("t", &scan.scan_id, "export", scan.revision),
        None,
    );
    assert_eq!(
        export::write_report(&service, &request(&scan, "json"), &service.repo.path, &job)
            .unwrap_err()
            .code,
        "APP_DATA_PROTECTED"
    );
    assert_eq!(
        service
            .repo
            .scan(&scan.scan_id)
            .unwrap()
            .logical_total_bytes,
        "3"
    );
}

#[test]
fn bounded_progress_coalesces_slow_consumer() {
    let delivered = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = delivered.clone();
    let job = JobRuntime::new(
        JobEvent::new("j", "s", "scan", 1),
        Some(Arc::new(move |e| sink.lock().unwrap().push(e))),
    );
    for _ in 0..100 {
        job.update(|e| e.status = "running".into());
    }
    assert_eq!(delivered.lock().unwrap().len(), 1);
    assert_eq!(job.snapshot().seq, 100);
    job.acknowledge(1).unwrap();
    assert_eq!(delivered.lock().unwrap().len(), 2);
    assert_eq!(delivered.lock().unwrap()[1].seq, 100);
    job.update(|e| e.status = "completed".into());
    assert_eq!(delivered.lock().unwrap().len(), 3);
    assert_eq!(job.acknowledge(999).unwrap_err().code, "INVALID_ACK");
}

#[cfg(windows)]
#[test]
fn sparse_and_compressed_allocation_is_platform_measured() {
    let temp = tempfile::tempdir().unwrap();
    let sparse = temp.path().join("sparse.bin");
    let f = fs::File::create(&sparse).unwrap();
    let set = std::process::Command::new("fsutil")
        .args(["sparse", "setflag"])
        .arg(&sparse)
        .output()
        .unwrap();
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    f.set_len(64 * 1024 * 1024).unwrap();
    drop(f);
    let meta = platform::metadata(&sparse).unwrap();
    assert_eq!(meta.logical, 64 * 1024 * 1024);
    assert!(meta.allocated.unwrap() < meta.logical, "{meta:?}");
    let compressed = temp.path().join("compressed.bin");
    fs::write(&compressed, vec![0u8; 1024 * 1024]).unwrap();
    let compression = std::process::Command::new("compact")
        .args(["/c", "/i", "/q"])
        .arg(&compressed)
        .output()
        .unwrap();
    assert!(compression.status.success());
    let meta = platform::metadata(&compressed).unwrap();
    assert!(meta.allocated.unwrap() < meta.logical, "{meta:?}");
}

#[cfg(windows)]
#[test]
fn shell_special_suffixes_fail_closed_for_sources_targets_and_members() {
    let temp = tempfile::tempdir().unwrap();
    let root = platform::absolute(temp.path()).unwrap().join("source");
    let dest = root.parent().unwrap().join("target");
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::create_dir_all(&dest).unwrap();
    fs::write(root.join("collision"), b"ordinary distinct file").unwrap();
    fs::write(root.join("collision."), b"trailing dot distinct file").unwrap();
    fs::write(root.join("collision "), b"trailing space distinct file").unwrap();
    fs::write(root.join("nested/inside."), b"nested special file").unwrap();
    fs::create_dir_all(root.join("parent.")).unwrap();
    fs::write(root.join("parent./child"), b"special parent child").unwrap();
    let special_target = root.parent().unwrap().join("target.");
    fs::create_dir(&special_target).unwrap();
    let service = Service::new(&temp.path().join("database")).unwrap();
    let started = service.start_scan(root.clone(), None).unwrap();
    let scan = wait_scan(&service, &started.scan_id);
    for name in ["collision.", "collision ", "child", "nested"] {
        let node = find(&service, &scan, name);
        for kind in ["copy", "move", "trash"] {
            let plan = file_ops::plan(
                &service,
                operation_request(&scan, &node, kind, Some(&dest), None, "skip"),
            )
            .unwrap();
            assert_eq!(
                plan.items[0].error.as_ref().unwrap().code,
                "UNSUPPORTED_SHELL_PATH",
                "{plan:?}"
            );
        }
        if name != "nested" {
            let stored = service
                .repo
                .node(&scan.scan_id, &node.node_id, Some(scan.revision))
                .unwrap();
            for action in ["open", "reveal", "properties"] {
                assert_eq!(
                    file_ops::open_node(&stored.path, action).unwrap_err().code,
                    "UNSUPPORTED_SHELL_PATH"
                );
            }
            let plan = file_ops::plan(
                &service,
                operation_request(&scan, &node, "rename", None, Some("renamed"), "skip"),
            )
            .unwrap();
            assert_eq!(
                plan.items[0].error.as_ref().unwrap().code,
                "UNSUPPORTED_SHELL_PATH"
            );
        }
    }
    let ordinary = find(&service, &scan, "collision");
    let plan = file_ops::plan(
        &service,
        operation_request(
            &scan,
            &ordinary,
            "copy",
            Some(&special_target),
            None,
            "skip",
        ),
    )
    .unwrap();
    assert_eq!(
        plan.items[0].error.as_ref().unwrap().code,
        "UNSUPPORTED_SHELL_PATH"
    );
    assert_eq!(
        fs::read(root.join("collision")).unwrap(),
        b"ordinary distinct file"
    );
    assert_eq!(
        fs::read(root.join("collision.")).unwrap(),
        b"trailing dot distinct file"
    );
    assert_eq!(
        fs::read(root.join("collision ")).unwrap(),
        b"trailing space distinct file"
    );
    assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&special_target).unwrap().count(), 0);
}

#[test]
fn keyset_pages_have_no_gaps_or_duplicates_and_locate_target_for_every_sort() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("synthetic");
    fs::create_dir(&root).unwrap();
    let service = Service::new(&temp.path().join("db")).unwrap();
    service.repo.insert_scan("pages", &root, true).unwrap();
    let mut c = service.repo.connect().unwrap();
    let tx = c.transaction().unwrap();
    let mut meta = platform::metadata(&root).unwrap();
    insert_node(&tx, "pages", 1, None, &root, 0, &meta, None).unwrap();
    meta.kind = "file".into();
    meta.recurse = false;
    meta.logical = 123;
    for id in 2..242 {
        insert_node(
            &tx,
            "pages",
            id,
            Some(1),
            &root.join(format!("sample-{id:04}")),
            1,
            &meta,
            None,
        )
        .unwrap();
    }
    tx.execute("UPDATE nodes SET name=CASE id%6 WHEN 0 THEN 'same' WHEN 1 THEN 'SAME' WHEN 2 THEN 'Å.txt' WHEN 3 THEN 'å.txt' WHEN 4 THEN '中文🧭' ELSE name END,logical_total=id%7,modified=CASE WHEN id%2=0 THEN NULL ELSE '2026-10-12T01:02:03.456Z' END WHERE scan_id='pages' AND id<>1", []).unwrap();
    tx.commit().unwrap();
    let scan = service.repo.finish("pages", "completed").unwrap();
    for (sort, ordering) in [
        ("size_desc", "sort_size,name COLLATE NOCASE,id"),
        (
            "size_asc",
            "sort_size DESC,name COLLATE NOCASE DESC,id DESC",
        ),
        ("name_asc", "name COLLATE NOCASE,id"),
        ("name_desc", "name COLLATE NOCASE DESC,id DESC"),
        (
            "modified_desc",
            "COALESCE(modified,'') DESC,name COLLATE NOCASE,id",
        ),
    ] {
        let mut query = PageRequest {
            scan_id: scan.scan_id.clone(),
            revision: scan.revision,
            parent_id: Some("1".into()),
            sort: Some(sort.into()),
            cursor: None,
            page_size: Some(17),
            filter: None,
        };
        let mut ids = Vec::new();
        loop {
            let page = service.repo.page(&query, false).unwrap();
            assert_eq!(page.total_count, "240");
            ids.extend(
                page.nodes
                    .into_iter()
                    .map(|n| parse_id(&n.node_id).unwrap()),
            );
            query.cursor = page.next_cursor;
            if query.cursor.is_none() {
                break;
            }
        }
        let expected = c
            .prepare(&format!(
                "SELECT id FROM nodes WHERE scan_id='pages' AND parent_id=1 ORDER BY {ordering}"
            ))
            .unwrap()
            .query_map([], |r| r.get::<_, i64>(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(ids, expected, "sort {sort}");
        if sort != "modified_desc" {
            let plan = c.prepare(&format!("EXPLAIN QUERY PLAN SELECT id FROM nodes WHERE scan_id='pages' AND parent_id=1 ORDER BY {ordering} LIMIT 17")).unwrap().query_map([],|row|row.get::<_,String>(3)).unwrap().collect::<std::result::Result<Vec<_>,_>>().unwrap().join(";");
            assert!(!plan.contains("TEMP B-TREE"), "indexed sort {sort}: {plan}");
            assert!(
                plan.contains(if sort.starts_with("size") {
                    "nodes_parent_size_v3"
                } else {
                    "nodes_parent_name_v2"
                }),
                "indexed sort {sort}: {plan}"
            );
        }
        if sort != "modified_desc" {
            let page = service
                .repo
                .node_page("pages", &expected[219].to_string(), scan.revision, sort, 17)
                .unwrap();
            assert!(
                page.nodes
                    .iter()
                    .any(|n| n.node_id == expected[219].to_string()),
                "locate {sort}"
            );
        }
    }
    let indexes = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='nodes_parent_size_asc_v2'",[],|r|r.get::<_,i64>(0)).unwrap();
    assert_eq!(indexes, 0);
    let mut old_query = PageRequest {
        scan_id: "pages".into(),
        revision: scan.revision,
        parent_id: Some("1".into()),
        sort: Some("size_asc".into()),
        cursor: None,
        page_size: Some(17),
        filter: None,
    };
    let page = service.repo.page(&old_query, false).unwrap();
    let mut old: serde_json::Value =
        serde_json::from_str(page.next_cursor.as_ref().unwrap()).unwrap();
    old["key"] = serde_json::to_value(
        serde_json::to_string(&(
            "pages",
            scan.revision,
            Some(1i64),
            "size_asc",
            &Filter::default(),
        ))
        .unwrap(),
    )
    .unwrap();
    old_query.cursor = Some(old.to_string());
    assert_eq!(
        service.repo.page(&old_query, false).unwrap_err().code,
        "INVALID_CURSOR"
    );
    for sort in ["size_asc", "size_desc"] {
        old_query.sort = Some(sort.into());
        old_query.cursor = None;
        let page = service.repo.page(&old_query, false).unwrap();
        let valid: serde_json::Value =
            serde_json::from_str(page.next_cursor.as_ref().unwrap()).unwrap();
        for (field, value) in [("size", i64::MIN), ("size", -1), ("id", 0), ("id", -1)] {
            let mut malformed = valid.clone();
            malformed[field] = value.into();
            old_query.cursor = Some(malformed.to_string());
            assert_eq!(
                service.repo.page(&old_query, false).unwrap_err().code,
                "INVALID_CURSOR",
                "{sort}/{field}/{value}"
            );
        }
        let mut boundary = valid.clone();
        boundary["size"] = i64::MAX.into();
        boundary["id"] = i64::MAX.into();
        old_query.cursor = Some(boundary.to_string());
        assert!(
            service.repo.page(&old_query, false).is_ok(),
            "{sort} accepts nonnegative integer boundary"
        );
    }
}

#[test]
fn duplicate_size_index_migration_preserves_old_history_and_page_size() {
    let (_temp, service, scan) = fixture();
    let c = service.repo.connect().unwrap();
    c.execute_batch("CREATE INDEX nodes_parent_size_asc_v2 ON nodes(scan_id,parent_id,logical_total,name COLLATE NOCASE,id)").unwrap();
    fn rows(c: &rusqlite::Connection, table: &str) -> Vec<Vec<rusqlite::types::Value>> {
        let mut statement = c
            .prepare(&format!(
                "SELECT * FROM {table} ORDER BY {}",
                if table == "nodes" { "scan_id,id" } else { "id" }
            ))
            .unwrap();
        let count = statement.column_count();
        statement
            .query_map([], |r| {
                (0..count)
                    .map(|i| r.get::<_, rusqlite::types::Value>(i))
                    .collect()
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
    }
    let node_rows = rows(&c, "nodes");
    let scan_rows = rows(&c, "scans");
    let page_size = c
        .query_row("PRAGMA page_size", [], |r| r.get::<_, i64>(0))
        .unwrap();
    let reopened = Repository::new(&service.repo.data_dir).unwrap();
    let after = reopened.connect().unwrap();
    assert_eq!(rows(&after, "nodes"), node_rows);
    assert_eq!(rows(&after, "scans"), scan_rows);
    assert_eq!(
        after
            .query_row("PRAGMA page_size", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        page_size
    );
    assert_eq!(after.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='nodes_parent_size_asc_v2'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(
        reopened.scan(&scan.scan_id).unwrap().revision,
        scan.revision
    );
    assert_eq!(
        after
            .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[cfg(windows)]
#[test]
fn sqlite_uses_verified_ordinary_filename_and_concurrent_reads_preserve_history() {
    let (_temp, service, scan) = fixture();
    assert!(service
        .repo
        .path
        .as_os_str()
        .to_string_lossy()
        .starts_with("\\\\?\\"));
    let c = service.repo.connect().unwrap();
    let filename: String = c
        .query_row("PRAGMA database_list", [], |r| r.get(2))
        .unwrap();
    assert!(!filename.starts_with("\\\\"));
    assert_eq!(
        platform::metadata(&service.repo.path).unwrap().identity,
        platform::metadata(Path::new(&filename)).unwrap().identity
    );
    let root_before: Vec<u8> = c
        .query_row(
            "SELECT root_path FROM scans WHERE id=?1",
            [&scan.scan_id],
            |r| r.get(0),
        )
        .unwrap();
    drop(c);
    let barrier = Arc::new(std::sync::Barrier::new(9));
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let barrier = barrier.clone();
            let service = &service;
            let scan = &scan;
            scope.spawn(move || {
                barrier.wait();
                for _ in 0..200 {
                    let current = service.repo.scan(&scan.scan_id).unwrap();
                    assert_eq!(current.status, "completed");
                    assert_eq!(current.logical_total_bytes, "76");
                }
            });
        }
        let barrier = barrier.clone();
        let service = &service;
        let scan = &scan;
        scope.spawn(move || {
            let mut c = service.repo.connect().unwrap();
            barrier.wait();
            for _ in 0..400 {
                let _writer = service.writer.lock().unwrap();
                let tx = c.transaction().unwrap();
                // Durable writes while leaving all history values unchanged.
                tx.execute(
                    "UPDATE scans SET revision=revision WHERE id=?1",
                    [&scan.scan_id],
                )
                .unwrap();
                tx.execute(
                    "UPDATE nodes SET logical_total=logical_total WHERE scan_id=?1 AND id=1",
                    [&scan.scan_id],
                )
                .unwrap();
                tx.commit().unwrap();
                std::thread::yield_now();
            }
        });
    });
    let reopened = Repository::new(&service.repo.data_dir).unwrap();
    let after = reopened.scan(&scan.scan_id).unwrap();
    assert_eq!(
        serde_json::to_value(&after).unwrap(),
        serde_json::to_value(&scan).unwrap()
    );
    let c = reopened.connect().unwrap();
    assert_eq!(
        c.query_row(
            "SELECT root_path FROM scans WHERE id=?1",
            [&scan.scan_id],
            |r| r.get::<_, Vec<u8>>(0)
        )
        .unwrap(),
        root_before
    );
    assert_eq!(
        c.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[cfg(windows)]
#[test]
fn unsupported_database_locations_keep_their_original_files() {
    use std::os::windows::ffi::OsStrExt;
    let temp = tempfile::tempdir().unwrap();
    let base = platform::absolute(temp.path()).unwrap();
    for name in ["cache.", "cache ", "CON.txt", "COM¹.bin"] {
        let data = base.join(name);
        fs::create_dir_all(&data).unwrap();
        let original = data.join("spacefold.sqlite3");
        fs::write(&original, b"original database sentinel").unwrap();
        let error = match Repository::new(&data) {
            Ok(_) => panic!("unsupported directory opened: {name}"),
            Err(e) => e,
        };
        assert_eq!(error.code, "DATABASE_PATH_UNSUPPORTED");
        assert_eq!(fs::read(original).unwrap(), b"original database sentinel");
    }
    let long = base.join("a".repeat(100)).join("b".repeat(100));
    fs::create_dir_all(&long).unwrap();
    let original = long.join("spacefold.sqlite3");
    assert!(original.as_os_str().encode_wide().count() > 251);
    fs::write(&original, b"long original database sentinel").unwrap();
    let error = match Repository::new(&long) {
        Ok(_) => panic!("over-budget database opened"),
        Err(e) => e,
    };
    assert_eq!(error.code, "DATABASE_PATH_UNSUPPORTED");
    assert_eq!(
        fs::read(original).unwrap(),
        b"long original database sentinel"
    );
}

#[test]
fn history_staleness_persists_without_rewriting_rows_and_root_refresh_clears_it() {
    let (temp, service, scan) = fixture();
    let loaded = service.repo.load_history(&scan.scan_id).unwrap();
    assert!(loaded.stale);
    assert_eq!(
        service.repo.load_history(&scan.scan_id).unwrap().revision,
        loaded.revision
    );
    assert_eq!(
        service
            .repo
            .connect()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE scan_id=?1 AND stale=1",
                [&scan.scan_id],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    let root = service
        .repo
        .node(&scan.scan_id, "1", Some(loaded.revision))
        .unwrap();
    assert!(root.node.stale);
    let target = temp.path().join("history.json");
    export::write_report(
        &service,
        &request(&loaded, "json"),
        &target,
        &JobRuntime::new(
            JobEvent::new("history", &scan.scan_id, "export", loaded.revision),
            None,
        ),
    )
    .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
    assert_eq!(report["metadata"]["stale"], true, "{report}");
    let refresh = service
        .refresh(scan.scan_id.clone(), "1".into(), None)
        .unwrap();
    let completed = wait_job(&service, &refresh.scan_id);
    assert_eq!(completed.status, "completed", "{completed:?}");
    let refreshed = service.repo.scan(&scan.scan_id).unwrap();
    assert!(!refreshed.stale);
    assert!(
        !service
            .repo
            .node(&scan.scan_id, "1", Some(refreshed.revision))
            .unwrap()
            .node
            .stale
    );
}

#[test]
fn compact_scan_ids_are_transactional_unique_never_reused_and_legacy_compatible() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Arc::new(Repository::new(&temp.path().join("db")).unwrap());
    repo.insert_scan("legacy-uuid-4e03-9af4", temp.path(), false)
        .unwrap();
    repo.insert_scan("s1", temp.path(), false).unwrap();
    let threads = (0..32)
        .map(|_| {
            let repo = repo.clone();
            std::thread::spawn(move || repo.new_scan_id().unwrap())
        })
        .collect::<Vec<_>>();
    let ids = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), 32);
    assert!(!ids.contains("s1"));
    let next = repo.new_scan_id().unwrap();
    repo.insert_scan(&next, temp.path(), false).unwrap();
    repo.connect()
        .unwrap()
        .execute("DELETE FROM scans WHERE id=?1", [&next])
        .unwrap();
    drop(repo);
    let reopened = Repository::new(&temp.path().join("db")).unwrap();
    let later = reopened.new_scan_id().unwrap();
    assert_ne!(later, next);
    assert!(!ids.contains(&later));
    assert_eq!(
        reopened.scan("legacy-uuid-4e03-9af4").unwrap().scan_id,
        "legacy-uuid-4e03-9af4"
    );
}

#[cfg(windows)]
#[test]
fn explicit_junction_root_is_rejected_before_canonicalization() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("real");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("untouched"), b"sentinel").unwrap();
    let link = temp.path().join("jump");
    assert!(std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(&root)
        .output()
        .unwrap()
        .status
        .success());
    let service = Service::new(&temp.path().join("db")).unwrap();
    assert_eq!(
        service.register_source(link.clone()).unwrap_err().code,
        "UNSAFE_SCAN_ROOT"
    );
    assert_eq!(
        service
            .resolve_source(None, Some(link.to_str().unwrap()))
            .unwrap_err()
            .code,
        "UNSAFE_SCAN_ROOT"
    );
    assert_eq!(fs::read(root.join("untouched")).unwrap(), b"sentinel");
}

#[cfg(windows)]
#[test]
fn export_parent_replacement_preserves_both_targets() {
    let (temp, service, scan) = fixture();
    let parent = temp.path().join("reports");
    let moved = temp.path().join("moved-reports");
    fs::create_dir(&parent).unwrap();
    let target = parent.join("report.json");
    fs::write(&target, b"ORIGINAL").unwrap();
    let callback_parent = parent.clone();
    let callback_moved = moved.clone();
    let job = JobRuntime::new(
        JobEvent::new("race", &scan.scan_id, "export", scan.revision),
        Some(Arc::new(move |event| {
            if event.phase == "核验输出目录" {
                fs::rename(&callback_parent, &callback_moved).unwrap();
                fs::create_dir(&callback_parent).unwrap();
                fs::write(callback_parent.join("report.json"), b"REPLACEMENT").unwrap();
            }
        })),
    );
    job.bounded_progress.store(false, Ordering::Release);
    assert_eq!(
        export::write_report(&service, &request(&scan, "json"), &target, &job)
            .unwrap_err()
            .code,
        "SOURCE_CHANGED"
    );
    assert_eq!(fs::read(target).unwrap(), b"REPLACEMENT");
    assert_eq!(fs::read(moved.join("report.json")).unwrap(), b"ORIGINAL");
    assert_eq!(fs::read_dir(&moved).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn export_cleanup_follows_own_open_temp_when_temp_is_moved() {
    let (temp, service, scan) = fixture();
    let parent = temp.path().join("reports");
    let moved = temp.path().join("moved-temp");
    fs::create_dir(&parent).unwrap();
    fs::create_dir(&moved).unwrap();
    let target = parent.join("report.json");
    fs::write(&target, b"ORIGINAL").unwrap();
    let callback_parent = parent.clone();
    let callback_moved = moved.clone();
    let job = JobRuntime::new(
        JobEvent::new("temp-race", &scan.scan_id, "export", scan.revision),
        Some(Arc::new(move |event| {
            if event.phase == "流式写入报告" {
                let own_temp = fs::read_dir(&callback_parent)
                    .unwrap()
                    .map(|entry| entry.unwrap())
                    .find(|entry| entry.file_name().to_string_lossy().starts_with(".tmp"))
                    .unwrap();
                fs::rename(own_temp.path(), callback_moved.join(own_temp.file_name())).unwrap();
                fs::write(own_temp.path(), b"UNRELATED REPLACEMENT").unwrap();
            }
        })),
    );
    job.bounded_progress.store(false, Ordering::Release);
    assert_eq!(
        export::write_report(&service, &request(&scan, "json"), &target, &job)
            .unwrap_err()
            .code,
        "EXPORT_TEMP_CHANGED"
    );
    assert_eq!(fs::read(target).unwrap(), b"ORIGINAL");
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 2);
    let replacement = fs::read_dir(&parent)
        .unwrap()
        .map(|entry| entry.unwrap())
        .find(|entry| entry.file_name().to_string_lossy().starts_with(".tmp"))
        .unwrap();
    assert_eq!(
        fs::read(replacement.path()).unwrap(),
        b"UNRELATED REPLACEMENT"
    );
    assert_eq!(fs::read_dir(&moved).unwrap().count(), 0);
}

#[test]
fn active_application_data_and_its_ancestors_are_protected_in_operation_plans() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    let holder = root.join("holder");
    fs::create_dir_all(&holder).unwrap();
    fs::write(holder.join("visible"), b"abc").unwrap();
    let safe = root.join("safe/app-data");
    fs::create_dir_all(&safe).unwrap();
    fs::write(safe.join("safe-file"), b"safe").unwrap();
    let service = Service::new(&holder.join("app-data")).unwrap();
    let scan = service
        .start_scan(platform::absolute(&root).unwrap(), None)
        .unwrap();
    let scan = wait_scan(&service, &scan.scan_id);
    let folder = find(&service, &scan, "holder");
    let destination = temp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    for kind in ["copy", "move", "rename", "trash"] {
        let plan = file_ops::plan(
            &service,
            operation_request(
                &scan,
                &folder,
                kind,
                Some(&destination),
                Some("renamed"),
                "skip",
            ),
        )
        .unwrap();
        assert_eq!(
            plan.items[0]
                .error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("APP_DATA_PROTECTED"),
            "{plan:?}"
        );
    }
    let safe_folder = find(&service, &scan, "app-data");
    let plan = file_ops::plan(
        &service,
        operation_request(&scan, &safe_folder, "copy", Some(&holder), None, "replace"),
    )
    .unwrap();
    assert_eq!(
        plan.items[0]
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("APP_DATA_PROTECTED"),
        "{plan:?}"
    );
    assert_eq!(fs::read(holder.join("visible")).unwrap(), b"abc");
    assert!(service.repo.path.is_file());
    assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn repaired_acl_error_directory_can_refresh_and_updates_kind_and_ancestor_counts() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    let denied = root.join("denied");
    fs::create_dir_all(&denied).unwrap();
    fs::write(denied.join("child"), b"abc").unwrap();
    let principal = std::env::var("USERNAME").unwrap();
    let grant = std::process::Command::new("icacls")
        .arg(&denied)
        .args(["/deny", &format!("{principal}:(OI)(CI)(R)")])
        .output()
        .unwrap();
    assert!(grant.status.success(), "{grant:?}");
    struct RestoreAcl {
        path: std::path::PathBuf,
        principal: String,
    }
    impl Drop for RestoreAcl {
        fn drop(&mut self) {
            let _ = std::process::Command::new("icacls")
                .arg(&self.path)
                .args(["/remove:d", &self.principal])
                .output();
        }
    }
    let restore = RestoreAcl {
        path: denied.clone(),
        principal: principal.clone(),
    };
    let service = Service::new(&temp.path().join("db")).unwrap();
    let started = service
        .start_scan(platform::absolute(&root).unwrap(), None)
        .unwrap();
    let scan = wait_scan(&service, &started.scan_id);
    let node = find(&service, &scan, "denied");
    // A supported parent-directory enumeration proves the accounting type and
    // identity even when opening the child to complete metadata is denied.
    // The legacy route instead retains an explicitly unknown entry type.
    let directory_known = node.kind == "directory";
    assert!(directory_known || node.kind == "other", "{node:?}");
    assert_eq!(scan.status, "completed_with_errors");
    assert_eq!(scan.discovered_count, "2");
    assert_eq!(scan.error_count, "1");
    assert_eq!(node.error_code.as_deref(), Some("ACCESS_DENIED"));
    assert!(node.incomplete);
    assert!(node.link_count.is_none());
    assert_eq!(node.logical_bytes, "0");
    assert_eq!(node.logical_total_bytes, "0");
    assert_eq!(node.allocated_known_total_bytes, "0");
    assert_eq!(node.file_count, "0");
    assert_eq!(node.directory_count, "0");
    assert_eq!(node.children_count, "0");
    let expected_unknown = if directory_known { "0" } else { "1" };
    assert_eq!(node.allocated_unknown_count, expected_unknown);
    if directory_known {
        assert!(node.identity.is_some());
        assert_eq!(node.allocated_bytes.as_deref(), Some("0"));
    } else {
        assert!(node.identity.is_none());
        assert!(node.allocated_bytes.is_none());
    }
    let before = service.repo.node(&scan.scan_id, "1", None).unwrap().node;
    assert_eq!(before.logical_total_bytes, "0");
    assert_eq!(before.allocated_known_total_bytes, "0");
    assert_eq!(before.allocated_unknown_count, expected_unknown);
    assert_eq!(before.file_count, "0");
    assert_eq!(
        before.directory_count,
        if directory_known { "1" } else { "0" }
    );
    assert_eq!(before.children_count, "1");
    assert!(before.incomplete);
    let conn = service.repo.connect().unwrap();
    spacefold_lib::repository::check_directory_metadata_complete(&conn, &scan.scan_id).unwrap();
    drop(conn);
    drop(restore);
    let measured_folder = platform::metadata(&platform::absolute(&denied).unwrap()).unwrap();
    let measured_child =
        platform::metadata(&platform::absolute(&denied.join("child")).unwrap()).unwrap();
    let refresh = service
        .refresh(scan.scan_id.clone(), node.node_id.clone(), None)
        .unwrap();
    let event = wait_job(&service, &refresh.scan_id);
    assert_eq!(event.status, "completed", "{event:?}");
    let current = service.repo.scan(&scan.scan_id).unwrap();
    assert_eq!(current.status, "completed");
    assert_eq!(current.error_count, "0");
    assert_eq!(current.logical_total_bytes, "3");
    let folder = service
        .repo
        .node(&scan.scan_id, &node.node_id, Some(current.revision))
        .unwrap()
        .node;
    assert_eq!(folder.kind, "directory");
    assert!(!folder.incomplete);
    assert_eq!(folder.error_code, None);
    assert_eq!(folder.identity, measured_folder.identity);
    assert_eq!(
        folder.link_count,
        measured_folder.links.map(|n| n.to_string())
    );
    if directory_known {
        assert_eq!(folder.identity, node.identity);
    }
    assert_eq!(folder.children_count, "1");
    let root = service
        .repo
        .node(&scan.scan_id, "1", Some(current.revision))
        .unwrap()
        .node;
    assert_eq!(root.directory_count, "1");
    assert_eq!(root.file_count, "1");
    assert_eq!(root.children_count, "1");
    assert_eq!(root.logical_total_bytes, "3");
    assert_eq!(root.allocated_unknown_count, "0");
    assert_eq!(
        root.allocated_known_total_bytes,
        measured_child.allocated.unwrap().to_string()
    );
    assert!(!root.incomplete);
    let conn = service.repo.connect().unwrap();
    spacefold_lib::repository::check_directory_metadata_complete(&conn, &scan.scan_id).unwrap();
}

#[cfg(windows)]
#[test]
fn root_copy_is_real_and_returns_top_level_directory() {
    let (temp, service, scan) = fixture();
    let root = service
        .repo
        .node(&scan.scan_id, "1", Some(scan.revision))
        .unwrap()
        .node;
    let target = temp.path().join("destination");
    fs::create_dir(&target).unwrap();
    for kind in ["move", "rename", "trash"] {
        let plan = file_ops::plan(
            &service,
            operation_request(&scan, &root, kind, Some(&target), Some("renamed"), "skip"),
        )
        .unwrap();
        assert_eq!(
            plan.items[0].error.as_ref().unwrap().code,
            "SCAN_ROOT_PROTECTED"
        );
    }
    let plan = file_ops::plan(
        &service,
        operation_request(&scan, &root, "copy", Some(&target), None, "skip"),
    )
    .unwrap();
    assert!(plan.items[0].error.is_none(), "{plan:?}");
    let job = file_ops::execute(&service, &plan.plan_id, None).unwrap();
    let done = wait_job(&service, &job.job_id);
    let result = &done.results.as_ref().unwrap()[0];
    assert_eq!(result.status, "success", "{done:?}");
    // Windows TEMP may use an 8.3 alias while the operation returns its resolved
    // long path. Compare the actual directories, retaining the top-level target.
    assert_eq!(
        fs::canonicalize(result.destination.as_deref().unwrap()).unwrap(),
        fs::canonicalize(target.join("source")).unwrap()
    );
    assert_eq!(
        fs::read(target.join("source/中文🧭/nested/a.bin")).unwrap(),
        vec![2u8; 31]
    );
}
