use crate::{
    model::*,
    platform,
    repository::{aggregate_batch, insert_node, insert_node_shallow, Repository},
    service::{JobRuntime, Service},
};
use rusqlite::{params, OptionalExtension};
use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanProfile {
    metadata_wait_ms: f64,
    writer_wait_ms: f64,
    insert_ms: f64,
    aggregate_ms: f64,
    commit_ms: f64,
    finish_ms: f64,
    batches: usize,
}
fn milliseconds(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

pub fn scan_sync(service: &Service, scan_id: &str, root: &Path, job: &JobRuntime) -> Result<Scan> {
    let started = Instant::now();
    let mut profile = ScanProfile::default();
    let mut c = service.repo.connect()?;
    // This connection's only TEMP table is a bounded set of <=256 consumed IDs;
    // durable nodes/work queues retain the repository's WAL/NORMAL policy.
    c.execute_batch(
        "PRAGMA temp_store=MEMORY;CREATE TEMP TABLE consumed_dirs(id INTEGER PRIMARY KEY);",
    )?;
    let rootmeta = platform::metadata(root)?;
    if rootmeta.kind != "directory" {
        return Err(AppError::new("NOT_DIRECTORY", "扫描来源必须是目录"));
    }
    {
        let _writer = service.writer.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute("UPDATE scans SET status='running' WHERE id=?1", [scan_id])?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE scan_id=?1 AND id=1)",
            [scan_id],
            |r| r.get(0),
        )?;
        if !exists {
            insert_node(&tx, scan_id, 1, None, root, 0, &rootmeta, None)?;
        }
        tx.commit()?;
    }
    let (work_tx, work_rx) = crossbeam_channel::bounded::<(i64, i64, PathBuf, u32)>(2000);
    let (result_tx, result_rx) =
        crossbeam_channel::bounded::<(i64, i64, PathBuf, u32, Result<platform::Metadata>)>(2000);
    let mut handles = vec![];
    for _ in 0..4 {
        let receiver = work_rx.clone();
        let sender = result_tx.clone();
        handles.push(std::thread::spawn(move || {
            while let Ok((id, parent, path, depth)) = receiver.recv() {
                let metadata = platform::metadata(&path);
                if sender.send((id, parent, path, depth, metadata)).is_err() {
                    break;
                }
            }
        }));
    }
    drop(work_rx);
    let mut next_id = 2i64;
    let mut last_emit = Instant::now();
    let mut failed: Option<AppError> = None;
    let mut pending = 0usize;
    let mut consumed = 0usize;
    let mut batch_started = Instant::now();
    'directories: loop {
        if job.cancel.load(Ordering::Acquire) {
            break;
        }
        if consumed >= 256 || consumed > 0 && batch_started.elapsed() >= Duration::from_millis(100)
        {
            if let Err(error) =
                flush_batch(service, &mut c, scan_id, pending, &result_rx, &mut profile)
            {
                failed = Some(error);
                break;
            }
            pending = 0;
            consumed = 0;
            batch_started = Instant::now();
        }
        let next: Option<(i64, Vec<u8>, u32)> = c
            .query_row(
                "SELECT id,path,depth FROM pending_dirs WHERE scan_id=?1 AND id NOT IN consumed_dirs ORDER BY id DESC LIMIT 1",
                [scan_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((parent, path, depth)) = next else {
            // A batch can span sibling directories. Commit queued discoveries
            // before deciding that the durable directory work queue is exhausted.
            if pending > 0 {
                if let Err(error) =
                    flush_batch(service, &mut c, scan_id, pending, &result_rx, &mut profile)
                {
                    failed = Some(error);
                    break;
                }
                pending = 0;
                consumed = 0;
                batch_started = Instant::now();
                continue;
            }
            break;
        };
        let path = platform::decode_path(path);
        {
            let _writer = service.writer.lock().unwrap();
            c.execute("INSERT INTO consumed_dirs(id) VALUES(?1)", [parent])?;
        }
        consumed += 1;
        if depth >= 4096 {
            record_error(
                service,
                &c,
                scan_id,
                parent,
                &path,
                "enumerate",
                AppError::new("DEPTH_LIMIT", "目录深度达到 4096，未继续读取"),
            )?;
            continue;
        }
        let fresh = match platform::metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                record_error(
                    service,
                    &c,
                    scan_id,
                    parent,
                    &path,
                    "directory_revalidate",
                    e,
                )?;
                continue;
            }
        };
        if !fresh.recurse || fresh.kind != "directory" {
            record_error(
                service,
                &c,
                scan_id,
                parent,
                &path,
                "directory_revalidate",
                AppError::new(
                    "DIRECTORY_CHANGED",
                    "目录在扫描中变为跳转链接或不可本地枚举，未跟随",
                ),
            )?;
            continue;
        }
        let iterator = match platform::read_dir(&path) {
            Ok(v) => v,
            Err(e) => {
                record_error(service, &c, scan_id, parent, &path, "enumerate", e)?;
                continue;
            }
        };
        for entry in iterator {
            if job.cancel.load(Ordering::Acquire) {
                break 'directories;
            }
            match entry {
                Ok(entry) => {
                    let p = entry.path;
                    if p.starts_with(&service.repo.data_dir) || service.is_work_file(&p) {
                        continue;
                    }
                    let id = next_id;
                    next_id = next_id
                        .checked_add(1)
                        .ok_or_else(|| AppError::new("SIZE_OUT_OF_RANGE", "节点数量超出上限"))?;
                    // Windows enumeration already provides ordinary size/time/attributes.
                    // Only entries without usable enumeration metadata need worker syscalls.
                    let hint = entry.hint.filter(|m| m.kind == "file");
                    let queued = if let Some(metadata) = hint {
                        result_tx
                            .send((id, parent, p, depth + 1, Ok(metadata)))
                            .is_ok()
                    } else {
                        work_tx.send((id, parent, p, depth + 1)).is_ok()
                    };
                    if !queued {
                        failed = Some(AppError::new("WORKER_FAILED", "元数据工作线程已退出"));
                        break 'directories;
                    }
                    pending += 1;
                }
                Err(e) => record_error(service, &c, scan_id, parent, &path, "enumerate", e)?,
            }
            if pending >= 2000 || batch_started.elapsed() >= Duration::from_millis(100) {
                if pending > 0 {
                    if let Err(e) =
                        flush_batch(service, &mut c, scan_id, pending, &result_rx, &mut profile)
                    {
                        failed = Some(e);
                        break 'directories;
                    }
                    pending = 0;
                    consumed = 0;
                }
                if last_emit.elapsed() >= Duration::from_millis(100) {
                    emit_scan(service, scan_id, job, "running", "读取目录与元数据")?;
                    last_emit = Instant::now();
                }
                batch_started = Instant::now();
            }
        }
        if last_emit.elapsed() >= Duration::from_millis(100) {
            emit_scan(service, scan_id, job, "running", "读取目录与元数据")?;
            last_emit = Instant::now();
        }
    }
    // Cancellation keeps every already-discovered metadata result in the partial
    // snapshot. Errors keep the preceding committed revision and fail the job.
    if failed.is_none() && pending > 0 {
        if let Err(error) = flush_batch(service, &mut c, scan_id, pending, &result_rx, &mut profile)
        {
            failed = Some(error);
        }
    }
    // Workers must finish outstanding metadata calls before the state becomes cancelled.
    drop(work_tx);
    drop(result_tx);
    while result_rx.try_recv().is_ok() {}
    drop(result_rx);
    for handle in handles {
        if handle.join().is_err() {
            failed = Some(AppError::new("WORKER_FAILED", "元数据线程异常退出"));
        }
    }
    if let Some(error) = failed {
        return Err(error);
    }
    let scan = service.repo.scan(scan_id)?;
    let state = if job.cancel.load(Ordering::Acquire) {
        "cancelled"
    } else if scan.error_count != "0" {
        "completed_with_errors"
    } else {
        "completed"
    };
    let finish_started = Instant::now();
    let result = {
        let _writer = service.writer.lock().unwrap();
        service.repo.finish(scan_id, state)?
    };
    profile.finish_ms = milliseconds(finish_started);
    if std::env::var_os("SPACEFOLD_PROFILE").is_some() {
        eprintln!(
            "{}",
            serde_json::json!({"scanProfile":profile,"totalMs":milliseconds(started)})
        );
    }
    emit_scan(
        service,
        scan_id,
        job,
        state,
        if state == "cancelled" {
            "扫描已停止，保留已完成部分"
        } else {
            "扫描完成"
        },
    )?;
    Ok(result)
}
fn record_error(
    service: &Service,
    c: &rusqlite::Connection,
    scan_id: &str,
    id: i64,
    path: &Path,
    stage: &str,
    error: AppError,
) -> Result<()> {
    let _writer = service.writer.lock().unwrap();
    Repository::error_conn(
        c,
        scan_id,
        ScanError {
            node_id: Some(id.to_string()),
            display_path: platform::display(path),
            stage: stage.into(),
            error,
        },
    )
}
fn flush_batch(
    service: &Service,
    c: &mut rusqlite::Connection,
    scan_id: &str,
    count: usize,
    receiver: &crossbeam_channel::Receiver<(i64, i64, PathBuf, u32, Result<platform::Metadata>)>,
    profile: &mut ScanProfile,
) -> Result<()> {
    let metadata_started = Instant::now();
    let mut results = Vec::with_capacity(count);
    for _ in 0..count {
        results.push(
            receiver
                .recv()
                .map_err(|_| AppError::new("WORKER_FAILED", "元数据线程中断"))?,
        )
    }
    profile.metadata_wait_ms += milliseconds(metadata_started);
    let wait_started = Instant::now();
    let _writer = service.writer.lock().unwrap();
    profile.writer_wait_ms += milliseconds(wait_started);
    let tx = c.transaction()?;
    let insert_started = Instant::now();
    let mut aggregates = std::collections::BTreeMap::<i64, [i64; 6]>::new();
    for (id, parent, path, depth, result) in results {
        let totals = aggregates.entry(parent).or_default();
        totals[5] += 1;
        match result {
            Ok(m) => {
                totals[0] = crate::repository::checked_add(totals[0], integer(m.logical)?)?;
                totals[1] = crate::repository::checked_add(
                    totals[1],
                    m.allocated.map(integer).transpose()?.unwrap_or(0),
                )?;
                totals[2] += i64::from(m.allocated.is_none());
                totals[3] += i64::from(m.kind == "file");
                totals[4] += i64::from(m.kind == "directory");
                let cloud_skip = m.kind == "directory" && !m.recurse && m.cloud_state.is_some();
                let error = cloud_skip.then(|| {
                    AppError::new(
                        "CLOUD_DIRECTORY_SKIPPED",
                        "云目录可能需要召回；未枚举其内容",
                    )
                });
                insert_node_shallow(
                    &tx,
                    scan_id,
                    id,
                    Some(parent),
                    &path,
                    depth,
                    &m,
                    error.as_ref(),
                )?;
                if let Some(error) = error {
                    Repository::error_conn(
                        &tx,
                        scan_id,
                        ScanError {
                            node_id: Some(id.to_string()),
                            display_path: platform::display(&path),
                            stage: "cloud_local_only".into(),
                            error,
                        },
                    )?;
                }
            }
            Err(error) => {
                totals[2] += 1;
                let m = platform::Metadata {
                    kind: "other".into(),
                    logical: 0,
                    allocated: None,
                    modified: None,
                    identity: None,
                    links: None,
                    attributes: 0,
                    reparse_tag: 0,
                    link_type: None,
                    link_target: None,
                    cloud_state: None,
                    allocation_method: None,
                    recurse: false,
                };
                insert_node_shallow(
                    &tx,
                    scan_id,
                    id,
                    Some(parent),
                    &path,
                    depth,
                    &m,
                    Some(&error),
                )?;
                Repository::error_conn(
                    &tx,
                    scan_id,
                    ScanError {
                        node_id: Some(id.to_string()),
                        display_path: platform::display(&path),
                        stage: "metadata".into(),
                        error,
                    },
                )?;
            }
        }
    }
    profile.insert_ms += milliseconds(insert_started);
    let aggregate_started = Instant::now();
    for (parent, [logical, allocated, unknown, files, dirs, children]) in aggregates {
        aggregate_batch(
            &tx, scan_id, parent, logical, allocated, unknown, files, dirs, children,
        )?;
    }
    tx.execute(
        "UPDATE scans SET revision=revision+1 WHERE id=?1",
        [scan_id],
    )?;
    tx.execute(
        "DELETE FROM pending_dirs WHERE scan_id=?1 AND id IN consumed_dirs",
        [scan_id],
    )?;
    tx.execute("DELETE FROM consumed_dirs", [])?;
    profile.aggregate_ms += milliseconds(aggregate_started);
    let commit_started = Instant::now();
    tx.commit()?;
    profile.commit_ms += milliseconds(commit_started);
    profile.batches += 1;
    Ok(())
}
pub fn emit_scan(
    service: &Service,
    id: &str,
    job: &JobRuntime,
    status: &str,
    phase: &str,
) -> Result<()> {
    let s = service.repo.scan(id)?;
    job.update(|e| {
        e.status = status.into();
        e.phase = phase.into();
        e.revision = s.revision;
        e.discovered_count = s.discovered_count;
        e.logical_total_bytes = s.logical_total_bytes;
        e.error_count = s.error_count;
    });
    Ok(())
}

/// Replace a fully scanned staging subtree in a single transaction; readers see one revision.
pub fn replace_subtree(
    service: &Service,
    target_scan: &str,
    scope_id: &str,
    staging_scan: &str,
) -> Result<u32> {
    let _writer = service.writer.lock().unwrap();
    let mut c = service.repo.connect()?;
    let tx = c.transaction()?;
    let old = Repository::node_conn(&tx, target_scan, parse_id(scope_id)?)?;
    let new = Repository::node_conn(&tx, staging_scan, 1)?;
    let ancestors = Repository::ancestors_conn(&tx, target_scan, parse_id(scope_id)?)?;
    tx.execute_batch("CREATE TEMP TABLE refresh_ids(old_id INTEGER PRIMARY KEY,new_id INTEGER); CREATE TEMP TABLE refresh_old(path BLOB PRIMARY KEY,id INTEGER,identity TEXT,fingerprint TEXT); CREATE TEMP TABLE refreshed_nodes(id INTEGER PRIMARY KEY);")?;
    tx.execute("WITH RECURSIVE subtree(id) AS (SELECT ?2 UNION ALL SELECT n.id FROM subtree s CROSS JOIN nodes n ON n.scan_id=?1 AND n.parent_id=s.id) INSERT INTO refreshed_nodes SELECT id FROM subtree",params![target_scan,parse_id(scope_id)?])?;
    tx.execute("INSERT INTO refresh_old SELECT path,id,COALESCE(json_extract(payload,'$.fingerprint.identity'),json_extract(payload,'$.metadata.identity'),json_extract(payload,'$.i')),json_object('kind',kind,'logical',COALESCE(json_extract(payload,'$.fingerprint.logical'),json_extract(payload,'$.metadata.logical'),json_extract(payload,'$.b')),'modified',modified,'reparse_tag',COALESCE(json_extract(payload,'$.fingerprint.reparse_tag'),json_extract(payload,'$.metadata.reparse_tag'),json_extract(payload,'$.t'),0)) FROM nodes WHERE scan_id=?1 AND id IN refreshed_nodes",[target_scan])?;
    let max: i64 = tx.query_row(
        "SELECT COALESCE(MAX(id),0) FROM nodes WHERE scan_id=?1",
        [target_scan],
        |r| r.get(0),
    )?;
    tx.execute("INSERT INTO refresh_ids(old_id,new_id) SELECT n.id,CASE WHEN n.id=1 THEN ?2 ELSE COALESCE((SELECT old.id FROM refresh_old old WHERE old.path=n.path AND (old.identity=COALESCE(json_extract(n.payload,'$.fingerprint.identity'),json_extract(n.payload,'$.metadata.identity'),json_extract(n.payload,'$.i')) OR ((old.identity IS NULL OR COALESCE(json_extract(n.payload,'$.fingerprint.identity'),json_extract(n.payload,'$.metadata.identity'),json_extract(n.payload,'$.i')) IS NULL) AND json_extract(old.fingerprint,'$.kind')=n.kind AND json_extract(old.fingerprint,'$.logical')=COALESCE(json_extract(n.payload,'$.fingerprint.logical'),json_extract(n.payload,'$.metadata.logical'),json_extract(n.payload,'$.b')) AND json_extract(old.fingerprint,'$.modified') IS n.modified AND json_extract(old.fingerprint,'$.reparse_tag')=COALESCE(json_extract(n.payload,'$.fingerprint.reparse_tag'),json_extract(n.payload,'$.metadata.reparse_tag'),json_extract(n.payload,'$.t'),0)))),n.id+?3) END FROM nodes n WHERE n.scan_id=?1",params![staging_scan,parse_id(scope_id)?,max])?;
    tx.execute("DELETE FROM scan_errors WHERE scan_id=?1 AND CAST(json_extract(payload,'$.nodeId') AS INTEGER) IN refreshed_nodes",[target_scan])?;
    tx.execute("WITH RECURSIVE subtree(id) AS (SELECT id FROM nodes WHERE scan_id=?1 AND parent_id=?2 UNION ALL SELECT n.id FROM subtree s CROSS JOIN nodes n ON n.scan_id=?1 AND n.parent_id=s.id) DELETE FROM nodes WHERE scan_id=?1 AND id IN subtree",params![target_scan,parse_id(scope_id)?])?;
    // Unchanged paths and file identities retain IDs; replaced identities receive fresh IDs.
    let root_payload: String = tx.query_row(
        "SELECT payload FROM nodes WHERE scan_id=?1 AND id=1",
        [staging_scan],
        |r| r.get(0),
    )?;
    tx.execute("UPDATE nodes SET payload=json_set(?3,'$.node.depth',depth),logical_total=?4,allocated_total=?5,allocated_unknown=?6,file_count=?7,dir_count=?8,children_count=?9,incomplete=?10,stale=0,error_code=?11,modified=?12 WHERE scan_id=?1 AND id=?2",params![target_scan,parse_id(scope_id)?,root_payload,parse_bytes(&new.node.logical_total_bytes)?,parse_bytes(&new.node.allocated_known_total_bytes)?,parse_bytes(&new.node.allocated_unknown_count)?,parse_bytes(&new.node.file_count)?,parse_bytes(&new.node.directory_count)?,parse_bytes(&new.node.children_count)?,new.node.incomplete,new.node.error_code,new.node.modified_at])?;
    tx.execute("INSERT INTO nodes(scan_id,id,parent_id,path,payload,name,kind,extension,depth,modified,logical_total,allocated_total,allocated_unknown,file_count,dir_count,children_count,incomplete,stale,error_code,search_path) SELECT ?1,m.new_id,p.new_id,n.path,json_set(n.payload,'$.node.depth',n.depth+?3),n.name,n.kind,n.extension,n.depth+?3,n.modified,n.logical_total,n.allocated_total,n.allocated_unknown,n.file_count,n.dir_count,n.children_count,n.incomplete,0,n.error_code,n.search_path FROM nodes n JOIN refresh_ids m ON n.id=m.old_id JOIN refresh_ids p ON n.parent_id=p.old_id WHERE n.scan_id=?2 AND n.id<>1",params![target_scan,staging_scan,old.node.depth])?;
    for ancestor in ancestors.into_iter().rev() {
        let delta = parse_bytes(&new.node.logical_total_bytes)?
            - parse_bytes(&old.node.logical_total_bytes)?;
        let adelta = parse_bytes(&new.node.allocated_known_total_bytes)?
            - parse_bytes(&old.node.allocated_known_total_bytes)?;
        checked_aggregate(&ancestor.node, delta, adelta)?;
        tx.execute("UPDATE nodes SET logical_total=logical_total+?3,allocated_total=allocated_total+?4,allocated_unknown=allocated_unknown+?5,file_count=file_count+?6,dir_count=dir_count+?7 WHERE scan_id=?1 AND id=?2",params![target_scan,parse_id(&ancestor.node.node_id)?,delta,adelta,parse_bytes(&new.node.allocated_unknown_count)?-parse_bytes(&old.node.allocated_unknown_count)?,parse_bytes(&new.node.file_count)?-parse_bytes(&old.node.file_count)?,parse_bytes(&new.node.directory_count)?-parse_bytes(&old.node.directory_count)?])?;
        tx.execute("UPDATE nodes SET incomplete=CASE WHEN error_code IS NULL AND NOT EXISTS(SELECT 1 FROM nodes child WHERE child.scan_id=?1 AND child.parent_id=?2 AND child.incomplete=1) THEN 0 ELSE 1 END WHERE scan_id=?1 AND id=?2",params![target_scan,parse_id(&ancestor.node.node_id)?])?;
    }
    tx.execute("UPDATE scans SET history_stale=CASE WHEN ?3=1 THEN 0 ELSE history_stale END,revision=revision+1,stale=(CASE WHEN ?3=1 THEN 0 ELSE history_stale END OR EXISTS(SELECT 1 FROM nodes WHERE scan_id=?1 AND stale=1)),discovered=(SELECT COUNT(*) FROM nodes WHERE scan_id=?1),disk_json=?2 WHERE id=?1",params![target_scan,platform::disk_space(&service.repo.root_path(target_scan)?).map(|d|serde_json::to_string(&d)).transpose()?,parse_id(scope_id)?])?;
    // Replace issues from this subtree and retain issues from unaffected scopes.
    let mut stmt = tx.prepare("SELECT payload FROM scan_errors WHERE scan_id=?1")?;
    let texts = stmt
        .query_map([staging_scan], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);
    for text in texts {
        let mut error: ScanError = serde_json::from_str(&text)?;
        if let Some(id) = &error.node_id {
            let mapped: i64 = tx.query_row(
                "SELECT new_id FROM refresh_ids WHERE old_id=?1",
                [parse_id(id)?],
                |r| r.get(0),
            )?;
            error.node_id = Some(mapped.to_string());
        }
        Repository::error_conn(&tx, target_scan, error)?;
    }
    tx.execute("UPDATE scans SET error_count=(SELECT COUNT(*) FROM scan_errors WHERE scan_id=?1),status=CASE WHEN ?2=1 THEN (SELECT status FROM scans WHERE id=?3) ELSE status END,finished=CASE WHEN ?2=1 THEN ?4 ELSE finished END WHERE id=?1",params![target_scan,parse_id(scope_id)?,staging_scan,now()])?;
    tx.execute("UPDATE scans SET status=CASE WHEN error_count=0 AND status='completed_with_errors' THEN 'completed' WHEN error_count>0 AND status='completed' THEN 'completed_with_errors' ELSE status END WHERE id=?1",[target_scan])?;
    let revision = Repository::scan_conn(&tx, target_scan)?.revision;
    tx.execute("DELETE FROM scans WHERE id=?1", [staging_scan])?;
    tx.execute("DELETE FROM scan_errors WHERE scan_id=?1", [staging_scan])?;
    tx.commit()?;
    Ok(revision)
}
fn checked_aggregate(n: &Node, delta: i64, adelta: i64) -> Result<()> {
    crate::repository::checked_add(parse_bytes(&n.logical_total_bytes)?, delta)?;
    crate::repository::checked_add(parse_bytes(&n.allocated_known_total_bytes)?, adelta)?;
    Ok(())
}
