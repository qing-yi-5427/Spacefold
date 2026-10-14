// Test-only synchronous correctness reference, preserved from ecff9ca.
use crate::{
    model::*,
    platform,
    repository::{aggregate_batch, insert_node, insert_node_shallow, Repository},
    service::{JobRuntime, Service},
};
use rusqlite::OptionalExtension;
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
        let path = crate::stored_path::decode(path).map_err(crate::stored_path::app_error)?;
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
