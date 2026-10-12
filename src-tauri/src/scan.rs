use crate::{
    model::*,
    platform,
    repository::{
        aggregate_batch, encode_node, insert_node, EncodedNode, NodeInserter, Repository,
    },
    service::{JobRuntime, Service},
};
use rusqlite::params;
use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanProfile {
    #[serde(skip)]
    enabled: bool,
    enumerate_ms: f64,
    directory_queue_ms: f64,
    reader_wait_ms: f64,
    directory_check_ms: f64,
    encode_ms: f64,
    sql_insert_ms: f64,
    pending_insert_ms: f64,
    max_flush_ms: f64,
    peak_batch_entries: usize,
    peak_batch_path_bytes: usize,
    cache_write_pages: i32,
    cache_spill_pages: i32,
    cache_used_bytes: i32,
    page_size: i32,
    first_commit_ms: Option<f64>,
    #[serde(skip)]
    started: Option<Instant>,
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
type MetadataResult = (
    i64,
    i64,
    PathBuf,
    u32,
    Result<platform::Metadata>,
    Option<EncodedNode>,
);
// Regular enumeration results stay in a bounded local batch. Only metadata
// requiring filesystem calls travels to workers. Time/path limits also bound
// unusually long paths and keep partial results available during a scan.
const BATCH_ENTRIES: usize = 8000;
const BATCH_PATH_BYTES: usize = 4 * 1024 * 1024;
const DIRECTORY_PREFETCH: usize = 32;
const READER_CHUNK_ENTRIES: usize = 256;
const READER_CHUNK_BYTES: usize = 256 * 1024;
const READER_QUEUE_CHUNKS: usize = 32;
const READER_DATA_BYTES: usize = 8 * 1024 * 1024;

/// Charges all queued and currently consumed reader chunks together. The one
/// chunk still being constructed is separately limited by chunk entries/bytes.
#[derive(Default)]
struct ReaderBudget {
    state: std::sync::Mutex<(usize, bool)>,
    changed: std::sync::Condvar,
}
struct ReaderPermit {
    budget: std::sync::Arc<ReaderBudget>,
    bytes: usize,
}
impl ReaderBudget {
    fn acquire(
        self: &std::sync::Arc<Self>,
        bytes: usize,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Option<ReaderPermit> {
        let mut state = self.state.lock().unwrap();
        if bytes > READER_DATA_BYTES {
            return None;
        }
        while state.0 + bytes > READER_DATA_BYTES && !state.1 && !cancel.load(Ordering::Acquire) {
            state = self
                .changed
                .wait_timeout(state, Duration::from_millis(50))
                .unwrap()
                .0;
        }
        if state.1 || cancel.load(Ordering::Acquire) {
            return None;
        }
        state.0 += bytes;
        Some(ReaderPermit {
            budget: self.clone(),
            bytes,
        })
    }
    fn stop(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}
impl Drop for ReaderPermit {
    fn drop(&mut self) {
        self.budget.state.lock().unwrap().0 -= self.bytes;
        self.budget.changed.notify_one();
    }
}

#[derive(Default)]
struct ReaderTimings {
    enumerate_ms: f64,
    directory_check_ms: f64,
    encode_ms: f64,
}
#[cfg(test)]
#[derive(Clone)]
struct ReaderTestHook {
    path: PathBuf,
    entered: crossbeam_channel::Sender<()>,
    waiting: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
}
#[cfg(test)]
static READER_TEST_HOOK: std::sync::Mutex<Option<ReaderTestHook>> = std::sync::Mutex::new(None);
#[cfg(test)]
static METADATA_PANIC_PATH: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
#[cfg(test)]
static READER_PANIC_PATH: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
#[cfg(test)]
type DispatchHook = (
    PathBuf,
    crossbeam_channel::Sender<()>,
    crossbeam_channel::Receiver<()>,
);
#[cfg(test)]
static DISPATCH_TEST_HOOK: std::sync::Mutex<Option<DispatchHook>> = std::sync::Mutex::new(None);
fn metadata_owned_bytes(m: Option<&platform::Metadata>) -> usize {
    m.map_or(0, |m| {
        m.kind.capacity()
            + [
                &m.modified,
                &m.identity,
                &m.link_type,
                &m.link_target,
                &m.cloud_state,
                &m.allocation_method,
            ]
            .into_iter()
            .map(|value| value.as_ref().map_or(0, String::capacity))
            .sum::<usize>()
    })
}
enum DirectoryMessage {
    Entries {
        parent: i64,
        path: PathBuf,
        depth: u32,
        entries: Vec<(platform::DirectoryEntry, Option<EncodedNode>)>,
        permit: ReaderPermit,
    },
    Error {
        parent: i64,
        path: PathBuf,
        stage: &'static str,
        error: AppError,
    },
    Done(ReaderTimings),
    Fatal(AppError),
}

/// Two readers overlap enumeration/encoding with the sole SQLite writer. Only
/// already-persisted directory IDs are requested. Bounded chunks provide
/// backpressure, and no iterator/file handle crosses its owning thread.
fn read_directories(
    requests: crossbeam_channel::Receiver<(i64, PathBuf, u32)>,
    output: crossbeam_channel::Sender<DirectoryMessage>,
    cancel: &std::sync::atomic::AtomicBool,
    stop: &std::sync::atomic::AtomicBool,
    profile: bool,
    budget: std::sync::Arc<ReaderBudget>,
) {
    while let Ok((parent, path, depth)) = requests.recv() {
        if cancel.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
            break;
        }
        #[cfg(test)]
        {
            let panic_here = READER_PANIC_PATH
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|p| p == &path);
            if panic_here {
                panic!("injected directory reader failure");
            }
        }
        #[cfg(test)]
        {
            let hook = READER_TEST_HOOK
                .lock()
                .unwrap()
                .as_ref()
                .filter(|h| h.path == path)
                .cloned();
            if let Some(hook) = hook {
                hook.entered.send(()).unwrap();
                hook.release.recv().unwrap();
            }
        }
        let mut timings = ReaderTimings::default();
        let started = profile.then(Instant::now);
        let fresh = platform::metadata(&path);
        if let Some(started) = started {
            timings.directory_check_ms += milliseconds(started);
        }
        let fresh = match fresh {
            Ok(m) if m.recurse && m.kind == "directory" => Ok(m),
            Ok(_) => Err(AppError::new(
                "DIRECTORY_CHANGED",
                "目录在扫描中变为跳转链接或不可本地枚举，未跟随",
            )),
            Err(e) => Err(e),
        };
        if let Err(error) = fresh {
            if output
                .send(DirectoryMessage::Error {
                    parent,
                    path,
                    stage: "directory_revalidate",
                    error,
                })
                .is_err()
            {
                break;
            }
            if output.send(DirectoryMessage::Done(timings)).is_err() {
                break;
            }
            continue;
        }
        let started = profile.then(Instant::now);
        let iterator = platform::read_dir(&path);
        if let Some(started) = started {
            timings.enumerate_ms += milliseconds(started);
        }
        let mut iterator = match iterator {
            Ok(iterator) => iterator,
            Err(error) => {
                if output
                    .send(DirectoryMessage::Error {
                        parent,
                        path,
                        stage: "enumerate",
                        error,
                    })
                    .is_err()
                {
                    break;
                }
                if output.send(DirectoryMessage::Done(timings)).is_err() {
                    break;
                }
                continue;
            }
        };
        let mut entries = Vec::with_capacity(READER_CHUNK_ENTRIES);
        let mut bytes = entries.capacity()
            * std::mem::size_of::<(platform::DirectoryEntry, Option<EncodedNode>)>();
        loop {
            if cancel.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
                return;
            }
            let started = profile.then(Instant::now);
            let entry = iterator.next();
            if let Some(started) = started {
                timings.enumerate_ms += milliseconds(started);
            }
            let Some(entry) = entry else {
                break;
            };
            match entry {
                Ok(entry) => {
                    let started = profile.then(Instant::now);
                    let encoded = entry
                        .hint
                        .as_ref()
                        .filter(|m| m.kind == "file")
                        .map(|m| encode_node(&entry.path, m))
                        .transpose();
                    if let Some(started) = started {
                        timings.encode_ms += milliseconds(started);
                    }
                    let encoded = match encoded {
                        Ok(encoded) => encoded,
                        Err(error) => {
                            if output
                                .send(DirectoryMessage::Error {
                                    parent,
                                    path: entry.path,
                                    stage: "encode",
                                    error,
                                })
                                .is_err()
                            {
                                return;
                            }
                            continue;
                        }
                    };
                    bytes += entry.path.capacity()
                        + metadata_owned_bytes(entry.hint.as_ref())
                        + encoded.as_ref().map_or(0, EncodedNode::bytes);
                    entries.push((entry, encoded));
                }
                Err(error) => {
                    if output
                        .send(DirectoryMessage::Error {
                            parent,
                            path: path.clone(),
                            stage: "enumerate",
                            error,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
            if entries.len() >= READER_CHUNK_ENTRIES || bytes >= READER_CHUNK_BYTES {
                let Some(permit) = budget.acquire(bytes, cancel) else {
                    return;
                };
                if output
                    .send(DirectoryMessage::Entries {
                        parent,
                        path: path.clone(),
                        depth,
                        entries,
                        permit,
                    })
                    .is_err()
                {
                    return;
                }
                entries = Vec::with_capacity(READER_CHUNK_ENTRIES);
                bytes = entries.capacity()
                    * std::mem::size_of::<(platform::DirectoryEntry, Option<EncodedNode>)>();
            }
        }
        if !entries.is_empty() {
            let Some(permit) = budget.acquire(bytes, cancel) else {
                return;
            };
            if output
                .send(DirectoryMessage::Entries {
                    parent,
                    path,
                    depth,
                    entries,
                    permit,
                })
                .is_err()
            {
                break;
            }
        }
        if output.send(DirectoryMessage::Done(timings)).is_err() {
            break;
        }
    }
}

pub fn scan_sync(service: &Service, scan_id: &str, root: &Path, job: &JobRuntime) -> Result<Scan> {
    let started = Instant::now();
    let mut profile = ScanProfile {
        enabled: std::env::var_os("SPACEFOLD_PROFILE").is_some(),
        started: Some(started),
        ..Default::default()
    };
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
    let (result_tx, result_rx) = crossbeam_channel::bounded::<MetadataResult>(2000);
    let mut handles = vec![];
    for _ in 0..4 {
        let receiver = work_rx.clone();
        let sender = result_tx.clone();
        handles.push(std::thread::spawn(move || {
            while let Ok((id, parent, path, depth)) = receiver.recv() {
                let metadata = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    #[cfg(test)]
                    if METADATA_PANIC_PATH
                        .lock()
                        .unwrap()
                        .as_ref()
                        .is_some_and(|p| p == &path)
                    {
                        panic!("injected metadata worker failure");
                    }
                    platform::metadata(&path)
                }))
                .unwrap_or_else(|_| Err(AppError::new("WORKER_FAILED", "元数据工作线程异常")));
                if sender
                    .send((id, parent, path, depth, metadata, None))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
    drop(work_rx);
    drop(result_tx);
    let mut next_id = 2i64;
    let mut last_emit = Instant::now();
    let mut failed: Option<AppError> = None;
    let mut pending = 0usize;
    let mut results = Vec::<MetadataResult>::with_capacity(BATCH_ENTRIES);
    let mut batch_path_bytes = 0usize;
    let mut consumed = 0usize;
    let mut batch_started = Instant::now();
    let pipeline_result = std::thread::scope(|scope| {
        let (request_tx, request_rx) = crossbeam_channel::bounded(DIRECTORY_PREFETCH);
        let (directory_tx, directory_rx) = crossbeam_channel::bounded(READER_QUEUE_CHUNKS);
        let enabled = profile.enabled;
        let budget = std::sync::Arc::new(ReaderBudget::default());
        let reader_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut readers = Vec::with_capacity(2);
        for _ in 0..2 {
            let requests = request_rx.clone();
            let output = directory_tx.clone();
            let reader_budget = budget.clone();
            let stop = reader_stop.clone();
            readers.push(scope.spawn(move || -> Result<()> {
                let fault_output = output.clone();
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    read_directories(
                        requests,
                        output,
                        &job.cancel,
                        &stop,
                        enabled,
                        reader_budget.clone(),
                    )
                }));
                if outcome.is_err() {
                    stop.store(true, Ordering::Release);
                    reader_budget.stop();
                    let error = AppError::new("WORKER_FAILED", "枚举线程异常退出");
                    let _ = fault_output.send(DirectoryMessage::Fatal(error.clone()));
                    return Err(error);
                }
                Ok(())
            }));
        }
        drop(request_rx);
        drop(directory_tx);
        let result = (|| -> Result<()> {
            let mut in_flight = 0usize;
            let mut refill = true;
            'directories: loop {
                if job.cancel.load(Ordering::Acquire) {
                    break;
                }
                if reader_stop.load(Ordering::Acquire) {
                    return Err(AppError::new("WORKER_FAILED", "枚举线程异常退出"));
                }
                if consumed >= 256
                    || consumed > 0 && batch_started.elapsed() >= Duration::from_millis(100)
                {
                    flush_batch(
                        service,
                        &mut c,
                        scan_id,
                        pending,
                        &result_rx,
                        &mut results,
                        &mut profile,
                    )?;
                    pending = 0;
                    batch_path_bytes = 0;
                    consumed = 0;
                    batch_started = Instant::now();
                    refill = true;
                }
                if refill && in_flight < DIRECTORY_PREFETCH {
                    let started = Instant::now();
                    let limit = (DIRECTORY_PREFETCH - in_flight).min(256 - consumed);
                    let directories = {
                        let mut statement = c.prepare_cached("SELECT id,path,depth FROM pending_dirs WHERE scan_id=?1 AND id NOT IN consumed_dirs ORDER BY id DESC LIMIT ?2")?;
                        let found = statement
                            .query_map(params![scan_id, limit as i64], |r| {
                                Ok((
                                    r.get::<_, i64>(0)?,
                                    r.get::<_, Vec<u8>>(1)?,
                                    r.get::<_, u32>(2)?,
                                ))
                            })?
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                        found
                    };
                    for (parent, path, depth) in directories {
                        #[cfg(test)]
                        {
                            let hook = DISPATCH_TEST_HOOK
                                .lock()
                                .unwrap()
                                .as_ref()
                                .filter(|h| h.0 == root)
                                .cloned();
                            if let Some((_, entered, release)) = hook {
                                entered.send(()).unwrap();
                                release.recv().unwrap();
                            }
                        }
                        if job.cancel.load(Ordering::Acquire) {
                            break 'directories;
                        }
                        if reader_stop.load(Ordering::Acquire) {
                            return Err(AppError::new("WORKER_FAILED", "枚举线程异常退出"));
                        }
                        let path = platform::decode_path(path);
                        c.execute("INSERT INTO consumed_dirs(id) VALUES(?1)", [parent])?;
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
                        } else {
                            if request_tx.send((parent, path, depth)).is_err() {
                                if job.cancel.load(Ordering::Acquire) {
                                    break 'directories;
                                }
                                return Err(AppError::new("WORKER_FAILED", "枚举线程已退出"));
                            }
                            in_flight += 1;
                        }
                    }
                    profile.directory_queue_ms += milliseconds(started);
                    refill = false;
                }
                if in_flight == 0 {
                    if pending > 0 || !results.is_empty() || consumed > 0 {
                        flush_batch(
                            service,
                            &mut c,
                            scan_id,
                            pending,
                            &result_rx,
                            &mut results,
                            &mut profile,
                        )?;
                        pending = 0;
                        batch_path_bytes = 0;
                        consumed = 0;
                        batch_started = Instant::now();
                        refill = true;
                        continue;
                    }
                    break;
                }
                let wait_started = Instant::now();
                #[cfg(test)]
                {
                    let hook = READER_TEST_HOOK
                        .lock()
                        .unwrap()
                        .as_ref()
                        .filter(|h| h.path == root)
                        .cloned();
                    if let Some(hook) = hook {
                        hook.waiting.send(()).unwrap();
                    }
                }
                let received = directory_rx.recv_timeout(Duration::from_millis(50));
                profile.reader_wait_ms += milliseconds(wait_started);
                let message = match received {
                    Ok(message) => message,
                    Err(_) if job.cancel.load(Ordering::Acquire) => break,
                    Err(crossbeam_channel::RecvTimeoutError::Timeout)
                        if reader_stop.load(Ordering::Acquire) =>
                    {
                        return Err(AppError::new("WORKER_FAILED", "枚举线程异常退出"))
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        return Err(AppError::new("WORKER_FAILED", "枚举线程中断"))
                    }
                };
                match message {
                    DirectoryMessage::Entries {
                        parent,
                        path,
                        depth,
                        entries,
                        permit,
                    } => {
                        for (entry, encoded) in entries {
                            if job.cancel.load(Ordering::Acquire) {
                                break 'directories;
                            }
                            let p = entry.path;
                            if p.starts_with(&service.repo.data_dir) || service.is_work_file(&p) {
                                continue;
                            }
                            let id = next_id;
                            next_id = next_id.checked_add(1).ok_or_else(|| {
                                AppError::new("SIZE_OUT_OF_RANGE", "节点数量超出上限")
                            })?;
                            batch_path_bytes += p.capacity()
                                + metadata_owned_bytes(entry.hint.as_ref())
                                + encoded.as_ref().map_or(0, EncodedNode::bytes);
                            profile.peak_batch_path_bytes =
                                profile.peak_batch_path_bytes.max(batch_path_bytes);
                            if let Some(metadata) = entry.hint.filter(|m| m.kind == "file") {
                                results.push((id, parent, p, depth + 1, Ok(metadata), encoded));
                            } else {
                                work_tx.send((id, parent, p, depth + 1)).map_err(|_| {
                                    AppError::new("WORKER_FAILED", "元数据工作线程已退出")
                                })?;
                                pending += 1;
                            }
                            if results.len() + pending >= BATCH_ENTRIES
                                || pending >= 2000
                                || batch_path_bytes >= BATCH_PATH_BYTES
                                || batch_started.elapsed() >= Duration::from_millis(100)
                            {
                                flush_batch(
                                    service,
                                    &mut c,
                                    scan_id,
                                    pending,
                                    &result_rx,
                                    &mut results,
                                    &mut profile,
                                )?;
                                pending = 0;
                                batch_path_bytes = 0;
                                consumed = 0;
                                batch_started = Instant::now();
                                refill = true;
                            }
                        }
                        let _ = path;
                        drop(permit);
                    }
                    DirectoryMessage::Error {
                        parent,
                        path,
                        stage,
                        error,
                    } => record_error(service, &c, scan_id, parent, &path, stage, error)?,
                    DirectoryMessage::Done(timings) => {
                        in_flight -= 1;
                        profile.enumerate_ms += timings.enumerate_ms;
                        profile.directory_check_ms += timings.directory_check_ms;
                        profile.encode_ms += timings.encode_ms;
                        refill = true;
                    }
                    DirectoryMessage::Fatal(error) => return Err(error),
                }
                if last_emit.elapsed() >= Duration::from_millis(100) {
                    emit_scan(service, scan_id, job, "running", "读取目录与元数据")?;
                    last_emit = Instant::now();
                }
            }
            Ok(())
        })();
        // Dropping both channels wakes a reader blocked by backpressure, on
        // cancellation and on errors. Join it before flushing/finalizing state.
        drop(request_tx);
        drop(directory_rx);
        reader_stop.store(true, Ordering::Release);
        budget.stop();
        let mut reader_error = None;
        for reader in readers {
            match reader.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => reader_error = Some(error),
                Err(_) => reader_error = Some(AppError::new("WORKER_FAILED", "枚举线程异常退出")),
            }
        }
        if let Some(error) = reader_error {
            return Err(error);
        }
        result
    });
    if let Err(error) = pipeline_result {
        failed = Some(error);
    }
    // Cancellation keeps every already-discovered metadata result in the partial
    // snapshot. Errors keep the preceding committed revision and fail the job.
    if failed.is_none() && (pending > 0 || !results.is_empty()) {
        if let Err(error) = flush_batch(
            service,
            &mut c,
            scan_id,
            pending,
            &result_rx,
            &mut results,
            &mut profile,
        ) {
            failed = Some(error);
        }
    }
    // Workers must finish outstanding metadata calls before the state becomes cancelled.
    drop(work_tx);
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
    if profile.enabled {
        // sqlite3_db_status only reads counters on this owned connection.
        for (op, output) in [
            (
                rusqlite::ffi::SQLITE_DBSTATUS_CACHE_WRITE,
                &mut profile.cache_write_pages,
            ),
            (
                rusqlite::ffi::SQLITE_DBSTATUS_CACHE_SPILL,
                &mut profile.cache_spill_pages,
            ),
            (
                rusqlite::ffi::SQLITE_DBSTATUS_CACHE_USED,
                &mut profile.cache_used_bytes,
            ),
        ] {
            let mut high_water = 0;
            unsafe {
                rusqlite::ffi::sqlite3_db_status(c.handle(), op, output, &mut high_water, 0);
            }
        }
        profile.page_size = c.query_row("PRAGMA page_size", [], |r| r.get(0))?;
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
    receiver: &crossbeam_channel::Receiver<MetadataResult>,
    results: &mut Vec<MetadataResult>,
    profile: &mut ScanProfile,
) -> Result<()> {
    let flush_started = Instant::now();
    let metadata_started = Instant::now();
    for _ in 0..count {
        results.push(
            receiver
                .recv()
                .map_err(|_| AppError::new("WORKER_FAILED", "元数据线程中断"))?,
        )
    }
    profile.peak_batch_entries = profile.peak_batch_entries.max(results.len());
    profile.metadata_wait_ms += milliseconds(metadata_started);
    let wait_started = Instant::now();
    let _writer = service.writer.lock().unwrap();
    profile.writer_wait_ms += milliseconds(wait_started);
    let tx = c.transaction()?;
    let insert_started = Instant::now();
    let mut inserter = NodeInserter::new(&tx, profile.enabled)?;
    let mut aggregates = std::collections::BTreeMap::<i64, [i64; 6]>::new();
    // Enumeration assigns increasing stable IDs. Sort the exceptional worker
    // results back into that order so primary/index inserts remain sequential.
    results.sort_unstable_by_key(|entry| entry.0);
    for (id, parent, path, depth, result, encoded) in results.drain(..) {
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
                if let Some(encoded) = encoded {
                    inserter.insert_encoded(
                        scan_id,
                        id,
                        Some(parent),
                        depth,
                        &m,
                        error.as_ref(),
                        &encoded,
                    )?;
                } else {
                    inserter.insert(scan_id, id, Some(parent), &path, depth, &m, error.as_ref())?;
                }
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
                if error.code == "WORKER_FAILED" {
                    return Err(error);
                }
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
                inserter.insert(scan_id, id, Some(parent), &path, depth, &m, Some(&error))?;
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
    if let Some(timings) = &inserter.timings {
        profile.encode_ms += timings.encode_ms;
        profile.sql_insert_ms += timings.sql_ms;
        profile.pending_insert_ms += timings.pending_ms;
    }
    drop(inserter);
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
    profile.max_flush_ms = profile.max_flush_ms.max(milliseconds(flush_started));
    if profile.first_commit_ms.is_none() && profile.peak_batch_entries > 0 {
        profile.first_commit_ms = profile.started.map(milliseconds);
    }
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
    tx.execute("UPDATE nodes SET payload=json_set(?3,'$.node.depth',depth),logical_total=?4,allocated_total=?5,allocated_unknown=?6,file_count=?7,dir_count=?8,children_count=?9,incomplete=?10,stale=0,error_code=?11,modified=?12,kind=?13,extension=?14 WHERE scan_id=?1 AND id=?2",params![target_scan,parse_id(scope_id)?,root_payload,parse_bytes(&new.node.logical_total_bytes)?,parse_bytes(&new.node.allocated_known_total_bytes)?,parse_bytes(&new.node.allocated_unknown_count)?,parse_bytes(&new.node.file_count)?,parse_bytes(&new.node.directory_count)?,parse_bytes(&new.node.children_count)?,new.node.incomplete,new.node.error_code,new.node.modified_at,new.node.kind,new.node.extension])?;
    tx.execute("INSERT INTO nodes(scan_id,id,parent_id,path,payload,name,kind,extension,depth,modified,logical_total,allocated_total,allocated_unknown,file_count,dir_count,children_count,incomplete,stale,error_code,search_path) SELECT ?1,m.new_id,p.new_id,n.path,json_set(n.payload,'$.node.depth',n.depth+?3),n.name,n.kind,n.extension,n.depth+?3,n.modified,n.logical_total,n.allocated_total,n.allocated_unknown,n.file_count,n.dir_count,n.children_count,n.incomplete,0,n.error_code,n.search_path FROM nodes n JOIN refresh_ids m ON n.id=m.old_id JOIN refresh_ids p ON n.parent_id=p.old_id WHERE n.scan_id=?2 AND n.id<>1",params![target_scan,staging_scan,old.node.depth])?;
    for ancestor in ancestors.into_iter().rev() {
        let delta = parse_bytes(&new.node.logical_total_bytes)?
            - parse_bytes(&old.node.logical_total_bytes)?;
        let adelta = parse_bytes(&new.node.allocated_known_total_bytes)?
            - parse_bytes(&old.node.allocated_known_total_bytes)?;
        checked_aggregate(&ancestor.node, delta, adelta)?;
        tx.execute("UPDATE nodes SET logical_total=logical_total+?3,allocated_total=allocated_total+?4,allocated_unknown=allocated_unknown+?5,file_count=file_count+?6,dir_count=dir_count+?7 WHERE scan_id=?1 AND id=?2",params![target_scan,parse_id(&ancestor.node.node_id)?,delta,adelta,parse_bytes(&new.node.allocated_unknown_count)?-parse_bytes(&old.node.allocated_unknown_count)?,parse_bytes(&new.node.file_count)?-parse_bytes(&old.node.file_count)?,parse_bytes(&new.node.directory_count)?-parse_bytes(&old.node.directory_count)?+i64::from(new.node.kind=="directory")-i64::from(old.node.kind=="directory")])?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::Arc};

    fn terminal(service: &Service, id: &str) -> JobEvent {
        let started = Instant::now();
        loop {
            let job = service.job(id).unwrap();
            if job.terminal() {
                return job.snapshot();
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "job did not terminate"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn skipped_depth_limit_batch_does_not_hide_older_normal_directory() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        fs::create_dir_all(source.join("normal")).unwrap();
        fs::write(source.join("normal/live.txt"), b"abc").unwrap();
        for i in 0..32 {
            fs::create_dir(source.join(format!("limited-{i}"))).unwrap();
        }
        let root = platform::absolute(&source).unwrap();
        let service = Service::new(&fixture.path().join("data")).unwrap();
        let id = service.repo.new_scan_id().unwrap();
        service.repo.insert_scan(&id, &root, false).unwrap();
        let mut c = service.repo.connect().unwrap();
        let tx = c.transaction().unwrap();
        insert_node(
            &tx,
            &id,
            1,
            None,
            &root,
            0,
            &platform::metadata(&root).unwrap(),
            None,
        )
        .unwrap();
        tx.execute("DELETE FROM pending_dirs WHERE scan_id=?1 AND id=1", [&id])
            .unwrap();
        let normal = root.join("normal");
        insert_node(
            &tx,
            &id,
            100,
            Some(1),
            &normal,
            1,
            &platform::metadata(&normal).unwrap(),
            None,
        )
        .unwrap();
        for i in 0..32 {
            let path = root.join(format!("limited-{i}"));
            insert_node(
                &tx,
                &id,
                i + 101,
                Some(1),
                &path,
                4096,
                &platform::metadata(&path).unwrap(),
                None,
            )
            .unwrap();
        }
        tx.commit().unwrap();
        let job = JobRuntime::new(JobEvent::new(&id, &id, "scan", 1), None);
        let scan = scan_sync(&service, &id, &root, &job).unwrap();
        assert_eq!(scan.status, "completed_with_errors");
        assert_eq!(scan.error_count, "32");
        assert_eq!(scan.logical_total_bytes, "3");
        assert_eq!(scan.discovered_count, "35");
        let root_node = service.repo.node(&id, "1", None).unwrap().node;
        assert_eq!(root_node.file_count, "1");
        assert_eq!(root_node.directory_count, "33");
        let normal_node = service.repo.node(&id, "100", None).unwrap().node;
        assert_eq!(normal_node.children_count, "1");
        assert!(!normal_node.incomplete);
        let errors = service.repo.errors(&id, None, 100).unwrap().errors;
        assert!(errors.iter().all(|e| e.error.code == "DEPTH_LIMIT"));
    }

    #[test]
    fn cancellation_while_reader_and_empty_receiver_are_blocked_is_cancelled() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("live.txt"), b"abc").unwrap();
        let root = platform::absolute(&source).unwrap();
        let service = Service::new(&fixture.path().join("data")).unwrap();
        let (entered_tx, entered_rx) = crossbeam_channel::bounded(1);
        let (waiting_tx, waiting_rx) = crossbeam_channel::bounded(1);
        let (release_tx, release_rx) = crossbeam_channel::bounded(1);
        *READER_TEST_HOOK.lock().unwrap() = Some(ReaderTestHook {
            path: root.clone(),
            entered: entered_tx,
            waiting: waiting_tx,
            release: release_rx,
        });
        struct ClearHook;
        impl Drop for ClearHook {
            fn drop(&mut self) {
                *READER_TEST_HOOK.lock().unwrap() = None;
            }
        }
        let _hook = ClearHook;
        let scan = service.start_scan(root, None).unwrap();
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        waiting_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let committed = service.repo.scan(&scan.scan_id).unwrap();
        let started = Instant::now();
        service.cancel_job(&scan.scan_id).unwrap();
        release_tx.send(()).unwrap();
        let event = terminal(&service, &scan.scan_id);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(event.status, "cancelled");
        assert!(event.error.is_none());
        let final_scan = service.repo.scan(&scan.scan_id).unwrap();
        assert_eq!(final_scan.status, "cancelled");
        assert_eq!(final_scan.discovered_count, committed.discovered_count);
        assert_eq!(event.discovered_count, final_scan.discovered_count);
        assert_eq!(event.revision, final_scan.revision);
        assert!(final_scan.revision > committed.revision);
    }

    #[test]
    fn stopped_reader_byte_budget_wakes_backpressured_acquisition() {
        let budget = Arc::new(ReaderBudget::default());
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let permit = budget.acquire(READER_DATA_BYTES, &cancel).unwrap();
        let (waiting_tx, waiting_rx) = crossbeam_channel::bounded(1);
        let reader_budget = budget.clone();
        let reader_cancel = cancel.clone();
        let reader = std::thread::spawn(move || {
            waiting_tx.send(()).unwrap();
            reader_budget.acquire(1, &reader_cancel).is_none()
        });
        waiting_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        budget.stop();
        assert!(reader.join().unwrap());
        drop(permit);
        assert_eq!(budget.state.lock().unwrap().0, 0);
    }

    #[test]
    fn cancellation_before_dispatch_never_reports_closed_reader_as_failure() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        fs::create_dir(&source).unwrap();
        let root = platform::absolute(&source).unwrap();
        let (entered_tx, entered_rx) = crossbeam_channel::bounded(1);
        let (release_tx, release_rx) = crossbeam_channel::bounded(1);
        *DISPATCH_TEST_HOOK.lock().unwrap() = Some((root.clone(), entered_tx, release_rx));
        struct ClearHook;
        impl Drop for ClearHook {
            fn drop(&mut self) {
                *DISPATCH_TEST_HOOK.lock().unwrap() = None;
            }
        }
        let _hook = ClearHook;
        let service = Service::new(&fixture.path().join("data")).unwrap();
        let scan = service.start_scan(root, None).unwrap();
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        service.cancel_job(&scan.scan_id).unwrap();
        release_tx.send(()).unwrap();
        let event = terminal(&service, &scan.scan_id);
        assert_eq!(event.status, "cancelled");
        assert!(event.error.is_none());
        assert_eq!(event.discovered_count, "1");
        let persisted = service.repo.scan(&scan.scan_id).unwrap();
        assert_eq!(persisted.status, "cancelled");
        assert_eq!(event.revision, persisted.revision);
    }

    #[test]
    fn metadata_worker_panic_fails_without_missing_result_or_hanging() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        fs::create_dir_all(source.join("panic-directory")).unwrap();
        let root = platform::absolute(&source).unwrap();
        *METADATA_PANIC_PATH.lock().unwrap() = Some(root.join("panic-directory"));
        struct ClearPanic;
        impl Drop for ClearPanic {
            fn drop(&mut self) {
                *METADATA_PANIC_PATH.lock().unwrap() = None;
            }
        }
        let _hook = ClearPanic;
        let service = Service::new(&fixture.path().join("data")).unwrap();
        let scan = service.start_scan(root, None).unwrap();
        let event = terminal(&service, &scan.scan_id);
        assert_eq!(event.status, "failed");
        assert_eq!(event.error.unwrap().code, "WORKER_FAILED");
        let committed = service.repo.scan(&scan.scan_id).unwrap();
        assert_eq!(committed.status, "failed");
        assert_eq!(committed.discovered_count, "1");
    }

    #[test]
    fn reader_panic_with_idle_peer_fails_and_joins_both_readers() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("unread.txt"), b"abc").unwrap();
        let root = platform::absolute(&source).unwrap();
        *READER_PANIC_PATH.lock().unwrap() = Some(root.clone());
        struct ClearPanic;
        impl Drop for ClearPanic {
            fn drop(&mut self) {
                *READER_PANIC_PATH.lock().unwrap() = None;
            }
        }
        let _hook = ClearPanic;
        let service = Service::new(&fixture.path().join("data")).unwrap();
        let started = Instant::now();
        let scan = service.start_scan(root, None).unwrap();
        let event = terminal(&service, &scan.scan_id);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "idle peer must not retain output channel forever"
        );
        assert_eq!(event.status, "failed");
        assert_eq!(event.error.unwrap().code, "WORKER_FAILED");
        let committed = service.repo.scan(&scan.scan_id).unwrap();
        assert_eq!(committed.status, "failed");
        assert_eq!(committed.discovered_count, "1");
        assert_eq!(event.revision, committed.revision);
        assert_eq!(event.discovered_count, committed.discovered_count);
    }
}
