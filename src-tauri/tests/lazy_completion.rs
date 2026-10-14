use spacefold_lib::{
    export,
    model::*,
    platform,
    repository::{insert_node, Repository},
    scan,
    service::{JobRuntime, Service},
};
use std::{fs, io::Read, path::Path, sync::Arc};

fn insert_tree(service: &Service, root: &Path) -> String {
    let id = service.repo.new_scan_id().unwrap();
    service.repo.insert_scan(&id, root, false).unwrap();
    let mut c = service.repo.connect().unwrap();
    let tx = c.transaction().unwrap();
    fn walk(
        tx: &rusqlite::Transaction<'_>,
        scan: &str,
        path: &Path,
        parent: Option<i64>,
        depth: u32,
        next: &mut i64,
    ) {
        let id = *next;
        *next += 1;
        let metadata = platform::metadata(path).unwrap();
        insert_node(tx, scan, id, parent, path, depth, &metadata, None).unwrap();
        if metadata.recurse {
            let mut children = fs::read_dir(path)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>();
            children.sort();
            for child in children {
                walk(tx, scan, &child, Some(id), depth + 1, next);
            }
        }
    }
    walk(&tx, &id, root, None, 0, &mut 1);
    tx.commit().unwrap();
    id
}
fn fixture() -> (tempfile::TempDir, Arc<Service>, std::path::PathBuf, String) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    fs::create_dir_all(root.join("bad")).unwrap();
    fs::create_dir_all(root.join("good")).unwrap();
    fs::write(root.join("bad/file.bin"), b"bad").unwrap();
    fs::write(root.join("good/file.bin"), b"good").unwrap();
    let root = platform::absolute(&root).unwrap();
    let service = Service::new(&temp.path().join("db")).unwrap();
    let id = insert_tree(&service, &root);
    (temp, service, root, id)
}
fn raw_flag(repo: &Repository, id: &str, node: i64) -> i64 {
    repo.connect()
        .unwrap()
        .query_row(
            "SELECT incomplete FROM nodes WHERE scan_id=?1 AND id=?2",
            rusqlite::params![id, node],
            |row| row.get(0),
        )
        .unwrap()
}

fn unknown_links_legacy_payload(repo: &Repository, id: &str, node: i64) -> String {
    let stored = repo.node(id, &node.to_string(), None).unwrap();
    let metadata = platform::Metadata {
        kind: stored.fingerprint.kind,
        logical: stored.fingerprint.logical,
        allocated: stored
            .node
            .allocated_bytes
            .map(|value| value.parse().unwrap()),
        modified: stored.fingerprint.modified,
        identity: stored.fingerprint.identity,
        links: None,
        attributes: stored.fingerprint.attributes,
        reparse_tag: stored.fingerprint.reparse_tag,
        link_type: stored.node.link_type,
        link_target: stored.node.link_target,
        cloud_state: stored.node.cloud_state,
        allocation_method: stored.node.allocation_method,
        recurse: true,
    };
    format!(
        "{{\"v\":2,\"metadata\":{}}}",
        serde_json::to_string(&metadata).unwrap()
    )
}

#[test]
fn persisted_zero_one_and_deferred_flags_are_consistent_across_every_read_api() {
    let (_temp, service, _root, id) = fixture();
    let repo = &service.repo;
    let c = repo.connect().unwrap();
    c.execute("UPDATE nodes SET incomplete=CASE id WHEN 3 THEN 0 WHEN 4 THEN 1 ELSE incomplete END WHERE scan_id=?1", [&id]).unwrap();
    for status in [
        "queued",
        "running",
        "cancelling",
        "cancelled",
        "failed",
        "interrupted",
        "completed",
        "completed_with_errors",
    ] {
        c.execute(
            "UPDATE scans SET status=?2 WHERE id=?1",
            rusqlite::params![id, status],
        )
        .unwrap();
        let expected = !matches!(status, "completed" | "completed_with_errors");
        let revision = repo.scan(&id).unwrap().revision;
        assert_eq!(
            repo.node(&id, "1", Some(revision)).unwrap().node.incomplete,
            expected,
            "{status}"
        );
        assert!(!repo.node(&id, "3", None).unwrap().node.incomplete);
        assert!(repo.node(&id, "4", None).unwrap().node.incomplete);
        let page = repo
            .page(
                &PageRequest {
                    scan_id: id.clone(),
                    parent_id: None,
                    revision,
                    sort: Some("name_asc".into()),
                    filter: None,
                    cursor: None,
                    page_size: Some(200),
                },
                true,
            )
            .unwrap();
        assert_eq!(
            page.nodes
                .iter()
                .find(|n| n.node_id == "2")
                .unwrap()
                .incomplete,
            expected
        );
        assert_eq!(
            repo.ancestors(&id, "3", revision).unwrap()[0].incomplete,
            expected
        );
        assert_eq!(
            repo.chart(&id, "1", revision, 2, 20)
                .unwrap()
                .root
                .node
                .incomplete,
            expected
        );
        assert_eq!(
            repo.selection(&id, &["1".into()], revision).unwrap().nodes[0].incomplete,
            expected
        );
        let view = repo
            .view_snapshot(&ViewSnapshotRequest {
                scan_id: id.clone(),
                scope_id: "1".into(),
                expanded_ids: vec!["1".into()],
                selected_ids: vec!["1".into()],
                primary_id: Some("1".into()),
                sort: None,
                page_anchors: vec![],
            })
            .unwrap();
        assert_eq!(view.chart.root.node.incomplete, expected);
        assert_eq!(view.summary.nodes[0].incomplete, expected);
        assert_eq!(
            view.nodes
                .iter()
                .find(|n| n.node_id == "1")
                .unwrap()
                .incomplete,
            expected
        );
    }
}

#[test]
fn completion_uses_sparse_legacy_clear_and_does_not_rewrite_deferred_directories() {
    let (_temp, service, _root, id) = fixture();
    let c = service.repo.connect().unwrap();
    c.execute(
        "UPDATE nodes SET incomplete=1 WHERE scan_id=?1 AND id=3",
        [&id],
    )
    .unwrap();
    c.execute_batch("CREATE TRIGGER reject_bulk_directory_clear BEFORE UPDATE OF incomplete ON nodes WHEN OLD.incomplete=2 AND NEW.incomplete=0 BEGIN SELECT RAISE(ABORT,'normal directory was rewritten'); END;").unwrap();
    let mut statement = c.prepare("EXPLAIN QUERY PLAN UPDATE nodes INDEXED BY nodes_incomplete SET incomplete=CASE WHEN error_code IS NULL THEN 0 ELSE 1 END WHERE scan_id=?1 AND incomplete=1").unwrap();
    let plan = statement
        .query_map([&id], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        plan.iter().any(|line| line.contains("nodes_incomplete")),
        "{plan:?}"
    );
    let finished = service.repo.finish(&id, "completed").unwrap();
    assert!(!finished.incomplete);
    assert_eq!(raw_flag(&service.repo, &id, 3), 0);
    for node in [1, 2, 4] {
        assert_eq!(raw_flag(&service.repo, &id, node), 2);
        assert!(
            !service
                .repo
                .node(&id, &node.to_string(), None)
                .unwrap()
                .node
                .incomplete
        );
    }
    assert_eq!(c.query_row::<i64,_,_>("SELECT COUNT(*) FROM nodes INDEXED BY nodes_incomplete WHERE scan_id=?1 AND incomplete=1", [&id], |r|r.get(0)).unwrap(), 0);
}

#[test]
fn error_ancestors_are_explicit_and_repair_keeps_completed_siblings_complete() {
    let (_temp, service, root, id) = fixture();
    let c = service.repo.connect().unwrap();
    Repository::error_conn(
        &c,
        &id,
        ScanError {
            node_id: Some("3".into()),
            display_path: platform::display(&root.join("bad/file.bin")),
            stage: "enumerate".into(),
            error: AppError::new("ACCESS_DENIED", "fixture"),
        },
    )
    .unwrap();
    service.repo.finish(&id, "completed_with_errors").unwrap();
    for node in [1, 2, 3] {
        assert_eq!(raw_flag(&service.repo, &id, node), 1);
    }
    assert_eq!(raw_flag(&service.repo, &id, 4), 2);
    assert!(!service.repo.node(&id, "4", None).unwrap().node.incomplete);
    let staged = insert_tree(&service, &root.join("bad"));
    service.repo.finish(&staged, "completed").unwrap();
    scan::replace_subtree(&service, &id, "2", &staged).unwrap();
    let repaired = service.repo.scan(&id).unwrap();
    assert_eq!(repaired.status, "completed");
    assert_eq!(repaired.error_count, "0");
    for node in [1, 2, 3, 4, 5] {
        assert!(
            !service
                .repo
                .node(&id, &node.to_string(), None)
                .unwrap()
                .node
                .incomplete
        );
    }
    service.repo.load_history(&id).unwrap();
    let historic = service.repo.node(&id, "4", None).unwrap().node;
    assert!(historic.stale);
    assert!(!historic.incomplete);
}

#[test]
fn partial_replacement_preserves_unscanned_siblings_and_root_replacement_materializes_flags() {
    for status in ["cancelled", "failed", "interrupted"] {
        let (_temp, service, root, id) = fixture();
        service.repo.finish(&id, status).unwrap();
        let c = service.repo.connect().unwrap();
        for node in [2, 4] {
            c.execute(
                "INSERT INTO directory_metadata_pending VALUES(?1,?2)",
                rusqlite::params![id, node],
            )
            .unwrap();
            c.execute(
                "UPDATE nodes SET payload=?3 WHERE scan_id=?1 AND id=?2",
                rusqlite::params![
                    id,
                    node,
                    unknown_links_legacy_payload(&service.repo, &id, node)
                ],
            )
            .unwrap();
        }
        let staged = insert_tree(&service, &root.join("bad"));
        service.repo.finish(&staged, "completed").unwrap();
        scan::replace_subtree(&service, &id, "2", &staged).unwrap();
        assert_eq!(service.repo.scan(&id).unwrap().status, status);
        assert!(!service.repo.node(&id, "2", None).unwrap().node.incomplete);
        assert_eq!(raw_flag(&service.repo, &id, 2), 0);
        assert_eq!(raw_flag(&service.repo, &id, 4), 2);
        assert!(service.repo.node(&id, "4", None).unwrap().node.incomplete);
        assert!(service.repo.node(&id, "1", None).unwrap().node.incomplete);
        assert_eq!(
            c.query_row::<i64, _, _>(
                "SELECT id FROM directory_metadata_pending WHERE scan_id=?1",
                [&id],
                |r| r.get(0)
            )
            .unwrap(),
            4
        );
        c.execute("INSERT INTO directory_metadata_pending VALUES(?1,1)", [&id])
            .unwrap();
        c.execute(
            "UPDATE nodes SET payload=?2 WHERE scan_id=?1 AND id=1",
            rusqlite::params![id, unknown_links_legacy_payload(&service.repo, &id, 1)],
        )
        .unwrap();
        let staged = insert_tree(&service, &root);
        service.repo.finish(&staged, "completed").unwrap();
        scan::replace_subtree(&service, &id, "1", &staged).unwrap();
        assert_eq!(service.repo.scan(&id).unwrap().status, "completed");
        let c = service.repo.connect().unwrap();
        assert_eq!(
            c.query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM nodes WHERE scan_id=?1 AND incomplete<>0",
                [&id],
                |r| r.get(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            c.query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM directory_metadata_pending WHERE scan_id=?1",
                [&id],
                |r| r.get(0)
            )
            .unwrap(),
            0
        );
    }
}

#[test]
fn reopening_an_active_scan_retains_deferred_nodes_as_interrupted_and_incomplete() {
    let (temp, service, _root, id) = fixture();
    service
        .repo
        .connect()
        .unwrap()
        .execute("UPDATE scans SET status='running' WHERE id=?1", [&id])
        .unwrap();
    drop(service);
    let repo = Repository::new(&temp.path().join("db")).unwrap();
    assert_eq!(repo.scan(&id).unwrap().status, "interrupted");
    let node = repo.node(&id, "2", None).unwrap().node;
    assert!(repo.scan(&id).unwrap().stale);
    assert!(node.incomplete);
    assert_eq!(raw_flag(&repo, &id, 2), 2);
}

#[test]
fn four_formats_read_back_the_effective_directory_completion_flag() {
    for (status, expected) in [("completed", "complete"), ("cancelled", "partial")] {
        let (temp, service, _root, id) = fixture();
        let scan = service.repo.finish(&id, status).unwrap();
        for format in ["json", "csv", "html", "xlsx"] {
            let request = ExportRequest {
                scan_id: id.clone(),
                revision: scan.revision,
                scope: "selected".into(),
                scope_id: "1".into(),
                selected_ids: vec!["1".into()],
                recursive: false,
                fields: vec!["node_id".into(), "measurement_status".into()],
                format: format.into(),
                target_path: None,
                html_row_budget: Some(20),
            };
            let preview = export::preview(&service, &request).unwrap();
            assert_eq!(preview.sample[0]["measurement_status"], expected);
            let target = temp.path().join(format!("report.{format}"));
            let job = JobRuntime::new(JobEvent::new("export", &id, "export", scan.revision), None);
            assert_eq!(
                export::write_report(&service, &request, &target, &job).unwrap(),
                1
            );
            match format {
                "json" => {
                    let value: serde_json::Value =
                        serde_json::from_slice(&fs::read(target).unwrap()).unwrap();
                    assert_eq!(value["rows"][0]["measurement_status"], expected);
                }
                "csv" => {
                    let bytes = fs::read(target).unwrap();
                    let mut reader = csv::Reader::from_reader(&bytes[3..]);
                    let records = reader
                        .records()
                        .collect::<std::result::Result<Vec<_>, _>>()
                        .unwrap();
                    assert_eq!(&records[1][3], expected);
                }
                "html" => assert!(fs::read_to_string(target)
                    .unwrap()
                    .contains(&format!("<td>{expected}</td>"))),
                "xlsx" => {
                    let mut zip = zip::ZipArchive::new(fs::File::open(target).unwrap()).unwrap();
                    let mut xml = String::new();
                    zip.by_name("xl/worksheets/sheet2.xml")
                        .unwrap()
                        .read_to_string(&mut xml)
                        .unwrap();
                    assert!(xml.contains(&format!("<t>{expected}</t>")), "{xml}");
                }
                _ => unreachable!(),
            }
        }
    }
}
