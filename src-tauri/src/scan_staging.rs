//! Batch-local preparation. No staged reference is a public node.
use super::*;
use std::collections::BTreeMap;

pub(super) const HOT_DIRECTORY_LIMIT: usize = 32;

#[derive(Clone, Copy, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Mode {
    Off,
    #[default]
    Staged,
}
impl Mode {
    pub(super) fn from_override(value: Option<&str>) -> Self {
        if value == Some("off") {
            Self::Off
        } else {
            Self::Staged
        }
    }
    pub(super) fn enabled(self) -> bool {
        matches!(self, Self::Staged)
    }
}

#[derive(Clone, Copy)]
pub(super) struct StagedDirectory(pub(super) i64);
#[derive(Clone, Copy)]
pub(super) struct CommittedDirectory(pub(super) i64);
pub(super) enum ReadTask {
    Staged(StagedDirectory),
    Committed(CommittedDirectory),
}
impl ReadTask {
    pub(super) fn id(&self) -> i64 {
        match self {
            Self::Staged(id) => id.0,
            Self::Committed(id) => id.0,
        }
    }
}

/// Only fixed-width values are retained in addition to the already-budgeted
/// packet. At most 8000 rows; no path, payload or metadata is cloned here.
pub(super) struct PreparedBatch {
    pub(super) totals: Vec<[i64; 6]>,
    pub(super) frontier: BTreeMap<i64, [i64; 6]>,
    pub(super) discovered_extra: i64,
    pub(super) absorbed_metadata: usize,
    pub(super) absorbed_edges: usize,
}

impl PreparedBatch {
    pub(super) fn prepare(
        rows: &mut [MetadataResult],
        metadata: &mut Vec<DirectoryMetadataUpdate>,
    ) -> Result<Self> {
        if rows.len() > 8000 {
            return Err(AppError::new(
                "READER_PAYLOAD_BUDGET",
                "暂存节点超过批次上限",
            ));
        }
        rows.sort_unstable_by_key(|row| row.0);
        for pair in rows.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(AppError::new("WRITER_PROTOCOL", "暂存节点身份重复"));
            }
        }
        // Preflight every matching result before moving any metadata. Error and
        // unknown-link completions retain the existing SQL/error path verbatim.
        for update in metadata.iter() {
            if let Ok(index) = rows.binary_search_by_key(&update.id, |row| row.0) {
                let old = rows[index]
                    .4
                    .as_ref()
                    .map_err(|_| AppError::new("SOURCE_CHANGED", "暂存目录记录已改变"))?;
                if old.kind != "directory"
                    || old.logical != 0
                    || old.allocated != Some(0)
                    || old.identity.as_deref() != Some(&update.expected_identity)
                {
                    return Err(AppError::new(
                        "SOURCE_CHANGED",
                        "暂存目录原身份或统计语义已改变",
                    ));
                }
                if let Ok(value) = &update.result {
                    if value.kind != "directory"
                        || !value.recurse
                        || value.logical != 0
                        || value.allocated != Some(0)
                        || value.identity.as_deref() != Some(&update.expected_identity)
                    {
                        return Err(AppError::new(
                            "SOURCE_CHANGED",
                            "目录句柄身份与暂存记录不同",
                        ));
                    }
                }
            }
        }
        let mut absorbed_metadata = 0;
        let mut index = 0;
        while index < metadata.len() {
            let target = rows
                .binary_search_by_key(&metadata[index].id, |row| row.0)
                .ok();
            if let Some(target) = target.filter(|_| {
                metadata[index]
                    .result
                    .as_ref()
                    .is_ok_and(|value| value.links.is_some())
            }) {
                let update = metadata.remove(index);
                rows[target].4 = update.result;
                // A directory encoding is cheap and produced once by the sole
                // writer, after its final metadata is known.
                rows[target].5 = None;
                absorbed_metadata += 1;
            } else {
                index += 1;
            }
        }
        let mut totals = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            let own = match &row.4 {
                Ok(value) => [
                    integer(value.logical)?,
                    value.allocated.map(integer).transpose()?.unwrap_or(0),
                    i64::from(value.allocated.is_none()),
                    i64::from(value.kind == "file"),
                    0,
                    0,
                ],
                Err(_) => [0, 0, 1, 0, 0, 0],
            };
            totals.push(own);
        }
        let mut frontier = BTreeMap::<i64, [i64; 6]>::new();
        let mut absorbed_edges = 0;
        for index in (0..rows.len()).rev() {
            let row = &rows[index];
            let mut contribution = totals[index];
            contribution[4] = crate::repository::checked_add(
                contribution[4],
                i64::from(row.4.as_ref().is_ok_and(|value| value.kind == "directory")),
            )?;
            contribution[5] = 1;
            if let Ok(parent) = rows.binary_search_by_key(&row.1, |entry| entry.0) {
                if parent >= index
                    || !rows[parent]
                        .4
                        .as_ref()
                        .is_ok_and(|value| value.kind == "directory")
                {
                    return Err(AppError::new("WRITER_PROTOCOL", "暂存节点父链不合法"));
                }
                add(&mut totals[parent], contribution)?;
                absorbed_edges += 1;
            } else {
                add(frontier.entry(row.1).or_default(), contribution)?;
            }
        }
        Ok(Self {
            totals,
            frontier,
            discovered_extra: absorbed_edges as i64,
            absorbed_metadata,
            absorbed_edges,
        })
    }
}
fn add(target: &mut [i64; 6], delta: [i64; 6]) -> Result<()> {
    for index in 0..6 {
        target[index] = crate::repository::checked_add(target[index], delta[index])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn directory(id: i64, parent: i64, path: PathBuf) -> MetadataResult {
        let mut metadata = platform::metadata(&path).unwrap();
        metadata.links = None;
        metadata.allocated = Some(0);
        (id, parent, path, 1, Ok(metadata), None)
    }
    fn file(id: i64, parent: i64, path: PathBuf) -> MetadataResult {
        let metadata = platform::metadata(&path).unwrap();
        (id, parent, path, 2, Ok(metadata), None)
    }
    fn completion(row: &MetadataResult) -> DirectoryMetadataUpdate {
        DirectoryMetadataUpdate {
            id: row.0,
            expected_identity: row.4.as_ref().unwrap().identity.clone().unwrap(),
            result: platform::metadata(&row.2),
        }
    }
    fn save(
        service: &Service,
        root: &Path,
        staged: bool,
        rows: Vec<MetadataResult>,
        metadata: Vec<DirectoryMetadataUpdate>,
    ) -> (String, ScanProfile) {
        let id = service.repo.new_scan_id().unwrap();
        service.repo.insert_scan(&id, root, false).unwrap();
        let mut connection = service.repo.connect().unwrap();
        let tx = connection.transaction().unwrap();
        insert_node(
            &tx,
            &id,
            1,
            None,
            root,
            0,
            &platform::metadata(root).unwrap(),
            None,
        )
        .unwrap();
        tx.commit().unwrap();
        connection
            .execute_batch("CREATE TEMP TABLE consumed_dirs(id INTEGER PRIMARY KEY);")
            .unwrap();
        let mut profile = ScanProfile {
            staged_insert_mode: if staged { Mode::Staged } else { Mode::Off },
            ..Default::default()
        };
        let (_, receiver) = crossbeam_channel::bounded(0);
        let mut rows = rows;
        let mut metadata = metadata;
        flush_batch_with_errors(
            service,
            &mut connection,
            &id,
            0,
            &receiver,
            &mut rows,
            &mut metadata,
            &mut Vec::new(),
            &mut profile,
        )
        .unwrap();
        (id, profile)
    }
    fn stored(service: &Service, scan: &str) -> Vec<Vec<rusqlite::types::Value>> {
        let connection = service.repo.connect().unwrap();
        let mut statement = connection.prepare("SELECT id,parent_id,path,payload,name,kind,extension,depth,modified,logical_total,allocated_total,allocated_unknown,file_count,dir_count,children_count,incomplete,stale,error_code,search_path,sort_size FROM nodes WHERE scan_id=?1 ORDER BY id").unwrap();
        statement
            .query_map([scan], |row| (0..20).map(|index| row.get(index)).collect())
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn staged_sqlite_partial_subtree_matches_every_stored_field_and_discovered_count() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("source");
        fs::create_dir_all(root.join("branch/child")).unwrap();
        fs::write(root.join("branch/child/中文🙂.bin"), b"abcdef").unwrap();
        let root = platform::absolute(&root).unwrap();
        let service = Service::new(&temp.path().join("data")).unwrap();
        let make = || {
            let rows = vec![
                file(4, 3, root.join("branch/child/中文🙂.bin")),
                directory(3, 2, root.join("branch/child")),
                directory(2, 1, root.join("branch")),
            ];
            let metadata = vec![completion(&rows[1]), completion(&rows[2])];
            (rows, metadata)
        };
        let (rows, metadata) = make();
        let (baseline, _) = save(&service, &root, false, rows, metadata);
        let (rows, metadata) = make();
        let (candidate, profile) = save(&service, &root, true, rows, metadata);
        assert_eq!(stored(&service, &baseline), stored(&service, &candidate));
        assert_eq!(service.repo.scan(&baseline).unwrap().discovered_count, "4");
        assert_eq!(service.repo.scan(&candidate).unwrap().discovered_count, "4");
        assert_eq!(profile.staged_metadata_inserts, 2);
        assert_eq!(profile.staged_internal_edges, 2);
        assert_eq!(profile.aggregate_ancestor_rows, 1);
    }

    #[test]
    fn staged_preflight_rejects_identity_cycles_duplicates_and_overflow_before_sql() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("dir")).unwrap();
        let path = platform::absolute(&temp.path().join("dir")).unwrap();
        let mut row = directory(2, 1, path.clone());
        let mut update = completion(&row);
        update.expected_identity = "changed".into();
        assert_eq!(
            PreparedBatch::prepare(std::slice::from_mut(&mut row), &mut vec![update])
                .err()
                .unwrap()
                .code,
            "SOURCE_CHANGED"
        );
        assert!(row.4.as_ref().unwrap().links.is_none());
        let mut rows = vec![directory(2, 3, path.clone()), directory(3, 2, path.clone())];
        assert_eq!(
            PreparedBatch::prepare(&mut rows, &mut Vec::new())
                .err()
                .unwrap()
                .code,
            "WRITER_PROTOCOL"
        );
        let mut rows = vec![directory(2, 1, path.clone()), directory(2, 1, path.clone())];
        assert_eq!(
            PreparedBatch::prepare(&mut rows, &mut Vec::new())
                .err()
                .unwrap()
                .code,
            "WRITER_PROTOCOL"
        );
        let mut rows = vec![directory(2, 1, path.clone()), directory(3, 2, path)];
        rows[0].4.as_mut().unwrap().allocated = Some(i64::MAX as u64);
        rows[1].4.as_mut().unwrap().allocated = Some(1);
        assert_eq!(
            PreparedBatch::prepare(&mut rows, &mut Vec::new())
                .err()
                .unwrap()
                .code,
            "SIZE_OUT_OF_RANGE"
        );
        assert!(Mode::from_override(None).enabled());
        assert!(Mode::from_override(Some("ON")).enabled());
        assert!(!Mode::from_override(Some("off")).enabled());
        assert!(Mode::from_override(Some("on")).enabled());
    }

    #[test]
    fn staged_real_scanner_matches_off_with_more_than_32_directories_and_deep_late_messages() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("source");
        for index in 0..70 {
            let child = root.join(format!("directory-{index}"));
            fs::create_dir_all(&child).unwrap();
            fs::write(child.join("中文🙂.bin"), b"abcdef").unwrap();
            fs::write(child.join("empty.bin"), b"").unwrap();
        }
        let mut deep = root.join("deep");
        for _ in 0..12 {
            deep = deep.join("next");
            fs::create_dir_all(&deep).unwrap();
            fs::write(deep.join("small"), b"x").unwrap();
        }
        fs::hard_link(
            root.join("directory-0/中文🙂.bin"),
            root.join("hardlink.bin"),
        )
        .unwrap();
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            let name =
                std::ffi::OsString::from_wide(&[b'u' as u16, 0xd800, b'.' as u16, b'b' as u16]);
            fs::write(root.join(name), b"wide").unwrap();
        }
        let root = platform::absolute(&root).unwrap();
        let service = Service::new(&temp.path().join("data")).unwrap();
        let run = || {
            let id = service.repo.new_scan_id().unwrap();
            service.repo.insert_scan(&id, &root, false).unwrap();
            let job = JobRuntime::new(JobEvent::new(&id, &id, "scan", 1), None);
            let scan = writer_pipeline::scan(&service, &id, &root, &job, true, 4, 32).unwrap();
            assert!(job.terminal());
            let connection = service.repo.connect().unwrap();
            let mut statement = connection
                .prepare("SELECT id FROM nodes WHERE scan_id=?1")
                .unwrap();
            let ids = statement
                .query_map([&id], |row| row.get::<_, i64>(0))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap();
            let nodes = ids
                .iter()
                .map(|id| Repository::node_conn(&connection, &scan.scan_id, *id).unwrap())
                .collect::<Vec<_>>();
            let mut canonical = BTreeMap::new();
            for stored in &nodes {
                let mut node = stored.node.clone();
                let parent = node.parent_id.as_ref().map(|id| {
                    platform::encode_path(
                        &nodes.iter().find(|n| &n.node.node_id == id).unwrap().path,
                    )
                });
                node.node_id.clear();
                node.parent_id = None;
                canonical.insert(platform::encode_path(&stored.path), serde_json::json!({"node":node,"fingerprint":stored.fingerprint,"parent":parent}));
            }
            assert_eq!(
                connection
                    .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
                    .unwrap(),
                "ok"
            );
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM pending_dirs WHERE scan_id=?1",
                        [&id],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                0
            );
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM directory_metadata_pending WHERE scan_id=?1",
                        [&id],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                0
            );
            (scan, canonical)
        };
        let (baseline, original) = run();
        let staged = writer_pipeline::test_staged_mode(&root);
        let (candidate, actual) = run();
        assert_eq!(original, actual);
        assert_eq!(baseline.discovered_count, candidate.discovered_count);
        assert_eq!(baseline.logical_total_bytes, candidate.logical_total_bytes);
        assert_eq!(candidate.error_count, "0");
        let observation = staged.observation();
        assert!(observation[0] > 0);
        assert!(observation[1] > 0);
        assert!(observation[2] > 0);
        assert!(observation[3] <= HOT_DIRECTORY_LIMIT);
    }

    #[test]
    fn staged_unknown_metadata_errors_and_nullable_mtime_keep_original_sql_semantics() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("source");
        fs::create_dir_all(root.join("directory")).unwrap();
        fs::write(root.join("directory/file"), b"abc").unwrap();
        let root = platform::absolute(&root).unwrap();
        let service = Service::new(&temp.path().join("data")).unwrap();
        for case in 0..4 {
            let make = || {
                let rows = vec![
                    directory(2, 1, root.join("directory")),
                    file(3, 2, root.join("directory/file")),
                ];
                let mut update = completion(&rows[0]);
                match case {
                    0 => update.result.as_mut().unwrap().links = None,
                    1 => update.result = Err(AppError::new("ACCESS_DENIED", "fixture denied")),
                    2 => update.result.as_mut().unwrap().modified = None,
                    _ => {
                        update.result.as_mut().unwrap().modified =
                            Some("2000-01-01T00:00:00Z".into())
                    }
                }
                (rows, vec![update])
            };
            let (rows, metadata) = make();
            let (baseline, _) = save(&service, &root, false, rows, metadata);
            let (rows, metadata) = make();
            let (candidate, _) = save(&service, &root, true, rows, metadata);
            assert_eq!(stored(&service, &baseline), stored(&service, &candidate));
            assert_eq!(
                service.repo.scan(&baseline).unwrap().error_count,
                service.repo.scan(&candidate).unwrap().error_count
            );
            let connection = service.repo.connect().unwrap();
            let errors = |id: &str| {
                let mut statement = connection
                    .prepare("SELECT payload FROM scan_errors WHERE scan_id=?1 ORDER BY id")
                    .unwrap();
                statement
                    .query_map([id], |row| row.get::<_, String>(0))
                    .unwrap()
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .unwrap()
            };
            assert_eq!(errors(&baseline), errors(&candidate));
        }
    }

    #[test]
    fn staged_late_metadata_and_child_packet_propagate_frontier_exactly_once() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("source");
        fs::create_dir_all(root.join("parent/child")).unwrap();
        fs::write(root.join("parent/child/file"), b"abcdef").unwrap();
        let root = platform::absolute(&root).unwrap();
        let service = Service::new(&temp.path().join("data")).unwrap();
        let run = |staged| {
            let parent = directory(2, 1, root.join("parent"));
            let late = completion(&parent);
            let (id, _) = save(&service, &root, staged, vec![parent], Vec::new());
            let child = directory(3, 2, root.join("parent/child"));
            let immediate = completion(&child);
            let mut rows = vec![file(4, 3, root.join("parent/child/file")), child];
            let mut metadata = vec![late, immediate];
            let mut connection = service.repo.connect().unwrap();
            connection
                .execute_batch("CREATE TEMP TABLE consumed_dirs(id INTEGER PRIMARY KEY);")
                .unwrap();
            let (_, receiver) = crossbeam_channel::bounded(0);
            let mut profile = ScanProfile {
                staged_insert_mode: if staged { Mode::Staged } else { Mode::Off },
                ..Default::default()
            };
            flush_batch_with_errors(
                &service,
                &mut connection,
                &id,
                0,
                &receiver,
                &mut rows,
                &mut metadata,
                &mut Vec::new(),
                &mut profile,
            )
            .unwrap();
            assert_eq!(service.repo.scan(&id).unwrap().discovered_count, "4");
            assert_eq!(
                service
                    .repo
                    .node(&id, "1", None)
                    .unwrap()
                    .node
                    .logical_total_bytes,
                "6"
            );
            assert_eq!(
                service
                    .repo
                    .node(&id, "2", None)
                    .unwrap()
                    .node
                    .children_count,
                "1"
            );
            (id, profile)
        };
        let (baseline, _) = run(false);
        let (candidate, profile) = run(true);
        assert_eq!(stored(&service, &baseline), stored(&service, &candidate));
        assert_eq!(profile.staged_metadata_inserts, 1);
        assert_eq!(profile.staged_internal_edges, 1);
    }

    #[test]
    fn staged_late_sql_failure_rolls_back_initial_totals_metadata_and_discovered() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("source");
        fs::create_dir_all(root.join("directory")).unwrap();
        fs::write(root.join("directory/file"), b"abc").unwrap();
        let root = platform::absolute(&root).unwrap();
        let service = Service::new(&temp.path().join("data")).unwrap();
        let (id, _) = save(&service, &root, true, Vec::new(), Vec::new());
        let before = stored(&service, &id);
        let mut connection = service.repo.connect().unwrap();
        connection.execute_batch("CREATE TEMP TABLE consumed_dirs(id INTEGER PRIMARY KEY); CREATE TEMP TRIGGER fixture_insert_failure BEFORE INSERT ON nodes WHEN NEW.id=3 BEGIN SELECT RAISE(ABORT,'fixture failure after parent insert'); END;").unwrap();
        let parent = directory(2, 1, root.join("directory"));
        let mut metadata = vec![completion(&parent)];
        let mut rows = vec![parent, file(3, 2, root.join("directory/file"))];
        let (_, receiver) = crossbeam_channel::bounded(0);
        let mut profile = ScanProfile {
            staged_insert_mode: Mode::Staged,
            ..Default::default()
        };
        assert!(flush_batch_with_errors(
            &service,
            &mut connection,
            &id,
            0,
            &receiver,
            &mut rows,
            &mut metadata,
            &mut Vec::new(),
            &mut profile
        )
        .is_err());
        assert_eq!(stored(&service, &id), before);
        assert_eq!(service.repo.scan(&id).unwrap().discovered_count, "1");
        assert!(!service.writer.is_poisoned());
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM directory_metadata_pending WHERE scan_id=?1",
                    [&id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
}
