#[cfg(test)]
use crate::repository::insert_node;
use crate::{
    model::*,
    platform,
    repository::{
        encode_node_with_codecs, DirectoryMetadataUpdate, EncodedNode, NodeInserter, Repository,
    },
    service::{JobRuntime, Service},
};
use rusqlite::params;
use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

#[path = "scan_checkpoint.rs"]
mod checkpoint;
#[path = "scan_staging.rs"]
mod staging;
#[path = "scan_pipeline.rs"]
mod writer_pipeline;

#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanProfile {
    #[serde(skip)]
    enabled: bool,
    #[serde(skip)]
    defer_commit_observation: bool,
    enumerate_ms: f64,
    directory_queue_ms: f64,
    directory_queue_read_ms: f64,
    directory_queue_decode_ms: f64,
    directory_queue_mark_ms: f64,
    directory_queue_send_ms: f64,
    directory_queue_error_ms: f64,
    directory_queue_queries: usize,
    directory_queue_read_rows: usize,
    directory_queue_peak_cached_entries: usize,
    directory_queue_peak_cached_bytes: usize,
    consumed_limit: usize,
    directory_readers: usize,
    directory_in_flight_limit: usize,
    writer_mode: &'static str,
    checkpoint_mode: &'static str,
    checkpoint_helper: checkpoint::Stats,
    output_slots: usize,
    output_slot_storage_upper_bound_bytes: usize,
    auxiliary_message_peak_bytes: usize,
    auxiliary_message_budget_waits: usize,
    directory_api: platform::DirectoryApiProfile,
    reader_wait_ms: f64,
    directory_check_ms: f64,
    encode_ms: f64,
    sql_insert_ms: f64,
    pending_insert_ms: f64,
    directory_metadata_apply_ms: f64,
    directory_metadata_updates: usize,
    directory_write_sql: crate::repository::DirectoryWriteSqlStats,
    directory_write_mode: crate::repository::DirectoryWriteMode,
    staged_insert_mode: staging::Mode,
    stored_path_codec: crate::stored_path::Mode,
    metadata_codec: crate::stored_metadata::Mode,
    file_path_storage: crate::stored_path::FileStorage,
    staged_directory_dispatches: usize,
    staged_directory_active_peak: usize,
    staged_ready_candidate_peak: usize,
    staged_metadata_inserts: usize,
    staged_internal_edges: usize,
    staged_counter_storage_upper_bound_bytes: usize,
    max_flush_ms: f64,
    peak_batch_entries: usize,
    peak_batch_path_bytes: usize,
    cache_write_pages: i32,
    cache_hit_pages: i32,
    cache_miss_pages: i32,
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
    aggregate_direct_parents: usize,
    aggregate_ancestor_rows: usize,
    aggregate_fallback_batches: usize,
    commit_ms: f64,
    writer_observation: crate::repository::WriterObservation,
    reader_backpressure: ReaderWaitStats,
    #[serde(skip)]
    reader_wait_probe: Option<std::sync::Arc<ReaderWaitProbe>>,
    finish_ms: f64,
    close_ms: f64,
    batches: usize,
    flush_consumed_limit: usize,
    flush_time_limit: usize,
    flush_entries_limit: usize,
    flush_metadata_limit: usize,
    flush_path_limit: usize,
    flush_drain: usize,
    flush_final: usize,
}
fn milliseconds(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}
type QueuedDirectory = (i64, Vec<u8>, u32, Option<String>, bool);
const FIRST_PENDING_DIRECTORIES: &str = "SELECT id,path,depth,expected_identity,metadata_deferred FROM pending_dirs WHERE scan_id=?1 AND id NOT IN consumed_dirs ORDER BY id DESC LIMIT ?2";
const NEXT_PENDING_DIRECTORIES: &str = "SELECT id,path,depth,expected_identity,metadata_deferred FROM pending_dirs WHERE scan_id=?1 AND id<?3 AND id NOT IN consumed_dirs ORDER BY id DESC LIMIT ?2";
const DIRECTORY_CACHE_BYTES: usize = 256 * 1024;
const DEFAULT_CONSUMED_LIMIT: usize = 1024;
fn consumed_limit_override(value: Option<&str>) -> usize {
    match value {
        Some("256") => 256,
        Some("1024") => 1024,
        Some("2048") => 2048,
        Some("4096") => 4096,
        _ => DEFAULT_CONSUMED_LIMIT,
    }
}
fn reader_count_for(value: Option<&str>, detected: usize) -> usize {
    match value {
        Some("2") => 2,
        Some("4") => 4,
        Some("8") => 8,
        Some("16") => 16,
        _ => detected.max(1),
    }
}
fn processor_count_from_providers(active: u32, fallback: impl FnOnce() -> usize) -> usize {
    if active != 0 {
        active as usize
    } else {
        fallback().max(1)
    }
}
fn detected_reader_count() -> usize {
    // Windows 11 schedules ordinary process threads across processor groups.
    // This read-only query counts system-active logical processors, rather
    // than estimating an affinity mask or a Job object's CPU quota.
    #[cfg(windows)]
    let active = unsafe {
        windows::Win32::System::Threading::GetActiveProcessorCount(
            windows::Win32::System::Threading::ALL_PROCESSOR_GROUPS,
        )
    };
    #[cfg(not(windows))]
    let active = 0;
    processor_count_from_providers(active, || {
        std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
    })
}
fn reader_count_override(value: Option<&str>) -> usize {
    reader_count_for(value, detected_reader_count())
}
fn overlapping_writer(value: Option<&str>) -> bool {
    value != Some("serial")
}
struct ReaderPlan {
    readers: usize,
    in_flight: usize,
    ledger_limit: usize,
}
impl ReaderPlan {
    fn new(readers: usize, consumed_limit: usize) -> Result<Self> {
        let unsupported = || AppError::new("RESOURCE_LIMIT", "可用核心数超出扫描资源支持范围");
        if readers == 0 {
            return Err(unsupported());
        }
        let in_flight = DIRECTORY_PREFETCH.max(readers);
        let ledger_limit = consumed_limit
            .checked_mul(2)
            .and_then(|n| n.checked_add(in_flight))
            .ok_or_else(unsupported)?;
        // Active requests, construction scratch and thread-handle storage grow
        // with hardware concurrency; all shared payload budgets remain fixed.
        let scratch = READER_DATA_BYTES
            .checked_add(READER_CHUNK_BYTES)
            .and_then(|n| n.checked_add(64 * 1024))
            .and_then(|n| {
                n.checked_add(
                    READER_CHUNK_ENTRIES
                        * std::mem::size_of::<(platform::DirectoryEntry, Option<EncodedNode>)>(),
                )
            })
            .ok_or_else(unsupported)?;
        readers
            .checked_mul(scratch)
            .filter(|n| *n <= isize::MAX as usize)
            .ok_or_else(unsupported)?;
        in_flight
            .checked_mul(std::mem::size_of::<(i64, PathBuf, u32, Option<String>, bool)>())
            .filter(|n| *n <= isize::MAX as usize)
            .ok_or_else(unsupported)?;
        Ok(Self {
            readers,
            in_flight,
            ledger_limit,
        })
    }
}
#[cfg(test)]
struct StartFault {
    root: PathBuf,
    role: &'static str,
    index: usize,
    active: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    started: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
#[cfg(test)]
static START_FAULT: std::sync::Mutex<Option<StartFault>> = std::sync::Mutex::new(None);
#[cfg(test)]
static START_FAULT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(test)]
struct ActiveWorker(std::sync::Arc<std::sync::atomic::AtomicUsize>);
#[cfg(test)]
impl Drop for ActiveWorker {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
#[cfg(test)]
fn start_probe(
    root: &Path,
    role: &str,
    index: usize,
) -> std::io::Result<
    Option<(
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    )>,
> {
    let fault = START_FAULT.lock().unwrap();
    if let Some(fault) = fault.as_ref().filter(|f| f.root == root) {
        if fault.role == role && fault.index == index {
            return Err(std::io::Error::other("injected thread creation failure"));
        }
        return Ok(Some((fault.active.clone(), fault.started.clone())));
    }
    Ok(None)
}
fn spawn_scoped_worker<'scope, 'env, F, T>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    root: &Path,
    role: &'static str,
    index: usize,
    work: F,
) -> std::io::Result<std::thread::ScopedJoinHandle<'scope, T>>
where
    F: FnOnce() -> T + Send + 'scope,
    T: Send + 'scope,
{
    #[cfg(test)]
    let active = start_probe(root, role, index)?;
    #[cfg(not(test))]
    let _ = root;
    std::thread::Builder::new()
        .name(format!("spacefold-{role}-{index}"))
        .spawn_scoped(scope, move || {
            #[cfg(test)]
            let _active = active.map(|(counter, started)| {
                started.fetch_add(1, Ordering::AcqRel);
                counter.fetch_add(1, Ordering::AcqRel);
                ActiveWorker(counter)
            });
            work()
        })
}
fn spawn_worker<F, T>(
    root: &Path,
    role: &'static str,
    index: usize,
    work: F,
) -> std::io::Result<std::thread::JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    #[cfg(test)]
    let active = start_probe(root, role, index)?;
    #[cfg(not(test))]
    let _ = root;
    std::thread::Builder::new()
        .name(format!("spacefold-{role}-{index}"))
        .spawn(move || {
            #[cfg(test)]
            let _active = active.map(|(counter, started)| {
                started.fetch_add(1, Ordering::AcqRel);
                counter.fetch_add(1, Ordering::AcqRel);
                ActiveWorker(counter)
            });
            work()
        })
}
fn thread_start_error(error: std::io::Error) -> AppError {
    AppError::new("WORKER_START_FAILED", format!("无法启动扫描线程：{error}"))
}
fn output_slots_override(value: Option<&str>) -> usize {
    match value {
        Some("32") => 32,
        Some("64") => 64,
        Some("128") => 128,
        _ => READER_QUEUE_CHUNKS,
    }
}

#[derive(Default)]
struct DirectoryQueueCursor {
    before_id: Option<i64>,
    cache: std::collections::VecDeque<QueuedDirectory>,
    payload_bytes: usize,
    exhausted: bool,
}
impl DirectoryQueueCursor {
    fn needs_refill(&self) -> bool {
        self.cache.is_empty() && !self.exhausted
    }
    fn owned_bytes(&self) -> usize {
        self.cache.capacity() * std::mem::size_of::<QueuedDirectory>() + self.payload_bytes
    }
    fn refill(&mut self, c: &rusqlite::Connection, scan_id: &str) -> Result<usize> {
        debug_assert!(self.needs_refill());
        self.cache.reserve(DIRECTORY_PREFETCH);
        let mut statement = c.prepare_cached(if self.before_id.is_some() {
            NEXT_PENDING_DIRECTORIES
        } else {
            FIRST_PENDING_DIRECTORIES
        })?;
        let mut rows = if let Some(before_id) = self.before_id {
            statement.query(params![scan_id, DIRECTORY_PREFETCH as i64, before_id])?
        } else {
            statement.query(params![scan_id, DIRECTORY_PREFETCH as i64])?
        };
        let mut read = 0;
        self.exhausted = true;
        while let Some(row) = rows.next()? {
            read += 1;
            let raw = row
                .get_ref(1)?
                .as_blob()
                .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?;
            let expected = row.get_ref(3)?;
            let expected = match expected {
                rusqlite::types::ValueRef::Null => None,
                value => Some(
                    value
                        .as_str()
                        .map_err(|error| AppError::new("DATABASE_ERROR", error.to_string()))?,
                ),
            };
            let payload_bytes = raw.len().saturating_add(expected.map_or(0, str::len));
            if self.cache.capacity() * std::mem::size_of::<QueuedDirectory>() + payload_bytes
                > BATCH_PATH_BYTES
            {
                return Err(AppError::new("PATH_LIMIT", "目录队列路径超出读取预算"));
            }
            if !self.cache.is_empty() && self.owned_bytes() + payload_bytes > DIRECTORY_CACHE_BYTES
            {
                self.exhausted = false;
                break;
            }
            let item = (
                row.get(0)?,
                raw.to_vec(),
                row.get(2)?,
                expected.map(str::to_owned),
                row.get(4)?,
            );
            self.payload_bytes += item.1.capacity() + item.3.as_ref().map_or(0, String::capacity);
            self.cache.push_back(item);
            if self.cache.len() == DIRECTORY_PREFETCH || self.owned_bytes() > DIRECTORY_CACHE_BYTES
            {
                // A single unusually long path is still dispatched; it cannot
                // make the cache's byte threshold stop forward progress.
                self.exhausted = false;
                break;
            }
        }
        Ok(read)
    }
    fn consume(
        &mut self,
        statement: &mut rusqlite::CachedStatement<'_>,
    ) -> Result<QueuedDirectory> {
        let id = self
            .cache
            .front()
            .ok_or_else(|| AppError::new("QUEUE_EMPTY", "目录队列为空"))?
            .0;
        statement.execute([id])?;
        self.before_id = Some(id);
        let item = self.cache.pop_front().unwrap();
        self.payload_bytes -= item.1.capacity() + item.3.as_ref().map_or(0, String::capacity);
        Ok(item)
    }
    fn reset_after_flush(&mut self) {
        // A committed batch can add newer directory IDs above the old watermark.
        self.before_id = None;
        self.cache.clear();
        self.payload_bytes = 0;
        self.exhausted = false;
    }
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
const AUXILIARY_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReaderWaitStats {
    byte_permit_waits: usize,
    message_send_waits: usize,
    // Metadata, Entries, Done, Error, Fatal.
    message_send_waits_by_kind: [usize; 5],
    byte_permit_wait_ms: f64,
    message_send_wait_ms: f64,
    flush_windows: usize,
    flush_window_ms: f64,
    readers_blocked_during_flush_ms: f64,
    byte_permit_blocked_during_flush_ms: f64,
    message_send_blocked_during_flush_ms: f64,
    byte_permit_waiter_ms_during_flush: f64,
    message_send_waiter_ms_during_flush: f64,
    commit_windows: usize,
    commit_window_ms: f64,
    readers_blocked_during_commit_ms: f64,
    byte_permit_blocked_during_commit_ms: f64,
    message_send_blocked_during_commit_ms: f64,
    byte_permit_waiter_ms_during_commit: f64,
    message_send_waiter_ms_during_commit: f64,
    peak_byte_permit_waiters: usize,
    peak_message_send_waiters: usize,
    active_waiters: [usize; 2],
}
#[derive(Default)]
struct ReaderWaitState {
    stats: ReaderWaitStats,
    last_transition: Option<Instant>,
    flush_active: bool,
    commit_active: bool,
}
impl ReaderWaitState {
    fn settle(&mut self, now: Instant) {
        if let Some(last) = self.last_transition {
            let elapsed = now.saturating_duration_since(last).as_secs_f64() * 1000.0;
            let [byte_waiters, send_waiters] = self.stats.active_waiters;
            self.stats.byte_permit_wait_ms += elapsed * byte_waiters as f64;
            self.stats.message_send_wait_ms += elapsed * send_waiters as f64;
            if self.flush_active {
                self.stats.flush_window_ms += elapsed;
                if byte_waiters + send_waiters > 0 {
                    self.stats.readers_blocked_during_flush_ms += elapsed;
                }
                if byte_waiters > 0 {
                    self.stats.byte_permit_blocked_during_flush_ms += elapsed;
                }
                if send_waiters > 0 {
                    self.stats.message_send_blocked_during_flush_ms += elapsed;
                }
                self.stats.byte_permit_waiter_ms_during_flush += elapsed * byte_waiters as f64;
                self.stats.message_send_waiter_ms_during_flush += elapsed * send_waiters as f64;
            }
            if self.commit_active {
                self.stats.commit_window_ms += elapsed;
                if byte_waiters + send_waiters > 0 {
                    self.stats.readers_blocked_during_commit_ms += elapsed;
                }
                if byte_waiters > 0 {
                    self.stats.byte_permit_blocked_during_commit_ms += elapsed;
                }
                if send_waiters > 0 {
                    self.stats.message_send_blocked_during_commit_ms += elapsed;
                }
                self.stats.byte_permit_waiter_ms_during_commit += elapsed * byte_waiters as f64;
                self.stats.message_send_waiter_ms_during_commit += elapsed * send_waiters as f64;
            }
        }
        self.last_transition = Some(now);
    }
}
#[derive(Clone, Copy)]
enum ReaderWaitRole {
    BytePermit,
    MessageSend(usize),
    Flush,
    Commit,
}
#[derive(Default)]
struct ReaderWaitProbe(std::sync::Mutex<ReaderWaitState>);
struct ReaderWaitSpan {
    probe: std::sync::Arc<ReaderWaitProbe>,
    role: ReaderWaitRole,
}
impl ReaderWaitProbe {
    fn begin(self: &std::sync::Arc<Self>, role: ReaderWaitRole) -> ReaderWaitSpan {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.settle(Instant::now());
        match role {
            ReaderWaitRole::Flush => {
                state.flush_active = true;
                state.stats.flush_windows += 1;
            }
            ReaderWaitRole::Commit => {
                state.commit_active = true;
                state.stats.commit_windows += 1;
            }
            ReaderWaitRole::BytePermit => {
                state.stats.byte_permit_waits += 1;
                state.stats.active_waiters[0] += 1;
                state.stats.peak_byte_permit_waiters = state
                    .stats
                    .peak_byte_permit_waiters
                    .max(state.stats.active_waiters[0]);
            }
            ReaderWaitRole::MessageSend(kind) => {
                state.stats.message_send_waits += 1;
                state.stats.message_send_waits_by_kind[kind] += 1;
                state.stats.active_waiters[1] += 1;
                state.stats.peak_message_send_waiters = state
                    .stats
                    .peak_message_send_waiters
                    .max(state.stats.active_waiters[1]);
            }
        }
        ReaderWaitSpan {
            probe: self.clone(),
            role,
        }
    }
    fn snapshot(&self) -> ReaderWaitStats {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.settle(Instant::now());
        state.stats.clone()
    }
}
impl Drop for ReaderWaitSpan {
    fn drop(&mut self) {
        let mut state = self
            .probe
            .0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.settle(Instant::now());
        match self.role {
            ReaderWaitRole::Flush => state.flush_active = false,
            ReaderWaitRole::Commit => state.commit_active = false,
            ReaderWaitRole::BytePermit => {
                state.stats.active_waiters[0] = state.stats.active_waiters[0].saturating_sub(1)
            }
            ReaderWaitRole::MessageSend(_) => {
                state.stats.active_waiters[1] = state.stats.active_waiters[1].saturating_sub(1)
            }
        }
    }
}

/// Charges all queued and currently consumed reader chunks together. Each of
/// each active reader's constructing chunk is separately limited by entries/bytes.
struct ReaderBudget {
    // Used bytes, stopped, high-water bytes, acquisition waits.
    state: std::sync::Mutex<(usize, bool, usize, usize)>,
    changed: std::sync::Condvar,
    observation: Option<std::sync::Arc<ReaderWaitProbe>>,
    limit: usize,
}
impl Default for ReaderBudget {
    fn default() -> Self {
        Self {
            state: Default::default(),
            changed: Default::default(),
            observation: None,
            limit: READER_DATA_BYTES,
        }
    }
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
        if bytes > self.limit {
            return None;
        }
        let mut waiting = None;
        let mut waited = false;
        while state.0 + bytes > self.limit && !state.1 && !cancel.load(Ordering::Acquire) {
            if !waited {
                waited = true;
                state.3 += 1;
                waiting = self
                    .observation
                    .as_ref()
                    .map(|probe| probe.begin(ReaderWaitRole::BytePermit));
            }
            state = self
                .changed
                .wait_timeout(state, Duration::from_millis(50))
                .unwrap()
                .0;
        }
        drop(waiting);
        if state.1 || cancel.load(Ordering::Acquire) {
            return None;
        }
        state.0 += bytes;
        state.2 = state.2.max(state.0);
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
    Metadata(DirectoryMetadataUpdate),
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
    Done(i64, ReaderTimings),
    Fatal(AppError),
}
impl DirectoryMessage {
    fn auxiliary_bytes(&self) -> usize {
        let error_bytes = |error: &AppError| {
            error.code.capacity()
                + error.message.capacity()
                + error.node_id.as_ref().map_or(0, String::capacity)
        };
        match self {
            Self::Metadata(update) => update.bytes(),
            Self::Entries { path, .. } => path.capacity(),
            Self::Error { path, error, .. } => path.capacity() + error_bytes(error),
            Self::Fatal(error) => error_bytes(error),
            Self::Done(..) => 0,
        }
    }
}
struct ReaderMessage {
    message: DirectoryMessage,
    auxiliary_permit: Option<ReaderPermit>,
}
struct ReaderOutput<'a> {
    sender: crossbeam_channel::Sender<ReaderMessage>,
    observation: Option<std::sync::Arc<ReaderWaitProbe>>,
    auxiliary_budget: std::sync::Arc<ReaderBudget>,
    cancel: &'a std::sync::atomic::AtomicBool,
    path_codec: crate::stored_path::Mode,
    metadata_codec: crate::stored_metadata::Mode,
    file_path_storage: crate::stored_path::FileStorage,
}
#[derive(Debug)]
enum EntryEncodingError {
    Skip,
    Stop,
}
impl ReaderOutput<'_> {
    fn encode_entry(
        &self,
        parent: i64,
        parent_path: &Path,
        entry: &platform::DirectoryEntry,
    ) -> std::result::Result<Option<EncodedNode>, EntryEncodingError> {
        let encoded = entry
            .scan_file_metadata()
            .or_else(|| entry.scan_directory_metadata())
            .map(|m| {
                if entry.scan_file_metadata().is_some()
                    && m.identity.is_some()
                    && m.allocation_method.as_deref()
                        == Some("FILE_ID_EXTD_DIR_INFO.AllocationSize")
                {
                    crate::repository::encode_native_file_with_parent(
                        &entry.path,
                        parent_path,
                        m,
                        self.path_codec,
                        self.metadata_codec,
                        self.file_path_storage,
                    )
                } else {
                    encode_node_with_codecs(&entry.path, m, self.path_codec, self.metadata_codec)
                }
            })
            .transpose();
        match encoded {
            Ok(encoded) => Ok(encoded),
            Err(error) if error.code == "STORED_METADATA_ENCODING" => {
                let _ = self.send(DirectoryMessage::Fatal(error));
                Err(EntryEncodingError::Stop)
            }
            Err(error) => match self.send(DirectoryMessage::Error {
                parent,
                path: entry.path.clone(),
                stage: "encode",
                error,
            }) {
                Ok(()) => Err(EntryEncodingError::Skip),
                Err(()) => Err(EntryEncodingError::Stop),
            },
        }
    }
    fn send_entries(
        &self,
        parent: i64,
        path: PathBuf,
        depth: u32,
        entries: Vec<(platform::DirectoryEntry, Option<EncodedNode>)>,
        bytes: usize,
        budget: &std::sync::Arc<ReaderBudget>,
    ) -> std::result::Result<(), ()> {
        if bytes > budget.limit {
            // Release the rejected owned chunk before waiting to report failure.
            // Otherwise a normal exit without Done leaves this request in flight.
            drop(entries);
            drop(path);
            if !self.cancel.load(Ordering::Acquire) {
                let _ = self.send_message(ReaderMessage {
                    message: DirectoryMessage::Fatal(AppError::new(
                        "READER_PAYLOAD_BUDGET",
                        "目录条目超过内存预算，已保留此前提交的扫描结果",
                    )),
                    auxiliary_permit: None,
                });
            }
            return Err(());
        }
        let permit = budget.acquire(bytes, self.cancel).ok_or(())?;
        self.send(DirectoryMessage::Entries {
            parent,
            path,
            depth,
            entries,
            permit,
        })
    }
    fn send(&self, message: DirectoryMessage) -> std::result::Result<(), ()> {
        let bytes = message.auxiliary_bytes();
        if bytes > self.auxiliary_budget.limit {
            drop(message);
            let _ = self.send_message(ReaderMessage {
                message: DirectoryMessage::Fatal(AppError::new(
                    "READER_PAYLOAD_BUDGET",
                    "目录消息超过内存预算，已保留此前提交的扫描结果",
                )),
                auxiliary_permit: None,
            });
            return Err(());
        }
        let auxiliary_permit = if bytes == 0 {
            None
        } else {
            Some(
                self.auxiliary_budget
                    .acquire(bytes, self.cancel)
                    .ok_or(())?,
            )
        };
        self.send_message(ReaderMessage {
            message,
            auxiliary_permit,
        })
    }
    fn send_message(&self, message: ReaderMessage) -> std::result::Result<(), ()> {
        let Some(probe) = &self.observation else {
            return self.sender.send(message).map_err(|_| ());
        };
        match self.sender.try_send(message) {
            Ok(()) => Ok(()),
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => Err(()),
            Err(crossbeam_channel::TrySendError::Full(message)) => {
                let kind = match &message.message {
                    DirectoryMessage::Metadata(_) => 0,
                    DirectoryMessage::Entries { .. } => 1,
                    DirectoryMessage::Done(..) => 2,
                    DirectoryMessage::Error { .. } => 3,
                    DirectoryMessage::Fatal(_) => 4,
                };
                let _waiting = probe.begin(ReaderWaitRole::MessageSend(kind));
                self.sender.send(message).map_err(|_| ())
            }
        }
    }
}
fn matches_directory_metadata(m: &platform::Metadata, expected: &Option<String>) -> bool {
    m.kind == "directory" && m.recurse && m.identity.as_ref() == expected.as_ref()
}
#[cfg(test)]
type DeferredDirectoryHook = (
    PathBuf,
    crossbeam_channel::Sender<()>,
    crossbeam_channel::Receiver<()>,
    crossbeam_channel::Sender<()>,
);
#[cfg(test)]
static DEFERRED_DIRECTORY_HOOK: std::sync::Mutex<Option<DeferredDirectoryHook>> =
    std::sync::Mutex::new(None);
#[cfg(test)]
#[derive(Clone)]
struct ParallelWorkHook {
    root: PathBuf,
    entered: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
}
#[cfg(test)]
static PARALLEL_WORK_HOOK: std::sync::Mutex<Option<ParallelWorkHook>> = std::sync::Mutex::new(None);

/// Directory readers overlap enumeration/encoding with the sole SQLite writer. Only
/// already-persisted directory IDs are requested. Bounded chunks provide
/// backpressure, and no iterator/file handle crosses its owning thread.
fn read_directories(
    requests: crossbeam_channel::Receiver<(i64, PathBuf, u32, Option<String>, bool)>,
    output: ReaderOutput<'_>,
    cancel: &std::sync::atomic::AtomicBool,
    stop: &std::sync::atomic::AtomicBool,
    profile: bool,
    budget: std::sync::Arc<ReaderBudget>,
    directory_reader: platform::ScanDirectoryReader,
) {
    while let Ok((parent, path, depth, expected_identity, deferred)) = requests.recv() {
        if cancel.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
            break;
        }
        #[cfg(test)]
        {
            let hook = PARALLEL_WORK_HOOK
                .lock()
                .unwrap()
                .as_ref()
                .filter(|hook| path.parent() == Some(hook.root.as_path()))
                .cloned();
            if let Some(hook) = hook {
                hook.entered.send(()).unwrap();
                hook.release.recv().unwrap();
            }
        }
        if deferred && expected_identity.is_none() {
            let _ = output.send(DirectoryMessage::Fatal(AppError::new(
                "METADATA_STATE_INVALID",
                "待补目录缺少原文件身份",
            )));
            return;
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
        let prepared = directory_reader.prepare(&path);
        if let Some(started) = started {
            timings.directory_check_ms += milliseconds(started);
        }
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                if deferred {
                    let measured = directory_reader.fallback_metadata(&path);
                    let changed = measured
                        .as_ref()
                        .is_ok_and(|m| !matches_directory_metadata(m, &expected_identity));
                    if !changed {
                        let failed = measured.is_err();
                        if output
                            .send(DirectoryMessage::Metadata(DirectoryMetadataUpdate {
                                id: parent,
                                expected_identity: expected_identity.clone().unwrap(),
                                result: measured,
                            }))
                            .is_err()
                        {
                            break;
                        }
                        if failed {
                            if output
                                .send(DirectoryMessage::Done(parent, timings))
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                    }
                }
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
                if output
                    .send(DirectoryMessage::Done(parent, timings))
                    .is_err()
                {
                    break;
                }
                continue;
            }
        };
        if deferred {
            #[cfg(test)]
            {
                let hook = DEFERRED_DIRECTORY_HOOK
                    .lock()
                    .unwrap()
                    .as_ref()
                    .filter(|h| h.0 == path)
                    .cloned();
                if let Some((_, entered, release, _)) = hook {
                    entered.send(()).unwrap();
                    release.recv().unwrap();
                }
            }
            let started = profile.then(Instant::now);
            let mut measured = prepared.directory_metadata();
            // Only a rare failed complete query uses the old metadata fallback;
            // the common directory stays on this one validated handle.
            if measured.is_err() {
                measured = directory_reader.fallback_metadata(&path);
            }
            if let Some(started) = started {
                timings.directory_check_ms += milliseconds(started);
            }
            let changed = measured
                .as_ref()
                .is_ok_and(|m| !matches_directory_metadata(m, &expected_identity));
            if changed {
                if output
                    .send(DirectoryMessage::Error {
                        parent,
                        path,
                        stage: "directory_revalidate",
                        error: AppError::new(
                            "DIRECTORY_CHANGED",
                            "目录的原文件身份已改变，未覆盖或枚举替代目录",
                        ),
                    })
                    .is_err()
                {
                    break;
                }
                if output
                    .send(DirectoryMessage::Done(parent, timings))
                    .is_err()
                {
                    break;
                }
                continue;
            }
            let failed = measured.is_err();
            if output
                .send(DirectoryMessage::Metadata(DirectoryMetadataUpdate {
                    id: parent,
                    expected_identity: expected_identity.unwrap(),
                    result: measured,
                }))
                .is_err()
            {
                break;
            }
            if failed {
                if output
                    .send(DirectoryMessage::Done(parent, timings))
                    .is_err()
                {
                    break;
                }
                continue;
            }
        }
        let started = profile.then(Instant::now);
        let iterator = prepared.entries();
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
                if output
                    .send(DirectoryMessage::Done(parent, timings))
                    .is_err()
                {
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
                    let encoded = output.encode_entry(parent, &path, &entry);
                    if let Some(started) = started {
                        timings.encode_ms += milliseconds(started);
                    }
                    let encoded = match encoded {
                        Ok(encoded) => encoded,
                        Err(EntryEncodingError::Stop) => return,
                        Err(EntryEncodingError::Skip) => continue,
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
                if output
                    .send_entries(parent, path.clone(), depth, entries, bytes, &budget)
                    .is_err()
                {
                    return;
                }
                entries = Vec::with_capacity(READER_CHUNK_ENTRIES);
                bytes = entries.capacity()
                    * std::mem::size_of::<(platform::DirectoryEntry, Option<EncodedNode>)>();
            }
        }
        if !entries.is_empty()
            && output
                .send_entries(parent, path, depth, entries, bytes, &budget)
                .is_err()
        {
            break;
        }
        if output
            .send(DirectoryMessage::Done(parent, timings))
            .is_err()
        {
            break;
        }
    }
}

pub fn scan_sync(service: &Service, scan_id: &str, root: &Path, job: &JobRuntime) -> Result<Scan> {
    // Diagnostic legacy switch changes enumeration only; persistence, budgets,
    // metadata completion and finish/close semantics stay identical.
    scan_sync_with_mode(
        service,
        scan_id,
        root,
        job,
        std::env::var_os("SPACEFOLD_LEGACY_ENUMERATOR").is_none(),
    )
}
fn scan_sync_with_mode(
    service: &Service,
    scan_id: &str,
    root: &Path,
    job: &JobRuntime,
    fast: bool,
) -> Result<Scan> {
    let readers =
        reader_count_override(std::env::var("SPACEFOLD_DIRECTORY_READERS").ok().as_deref());
    #[cfg(test)]
    let readers = START_FAULT
        .lock()
        .unwrap()
        .as_ref()
        .filter(|fault| fault.root == root)
        .map_or(readers, |_| 4);
    scan_sync_with_options(service, scan_id, root, job, fast, readers)
}
fn scan_sync_with_options(
    service: &Service,
    scan_id: &str,
    root: &Path,
    job: &JobRuntime,
    fast: bool,
    readers: usize,
) -> Result<Scan> {
    let output_slots =
        output_slots_override(std::env::var("SPACEFOLD_OUTPUT_SLOTS").ok().as_deref());
    scan_sync_with_queue_capacity(service, scan_id, root, job, fast, readers, output_slots)
}
fn scan_sync_with_queue_capacity(
    service: &Service,
    scan_id: &str,
    root: &Path,
    job: &JobRuntime,
    fast: bool,
    readers: usize,
    output_slots: usize,
) -> Result<Scan> {
    #[cfg(test)]
    let overlap = writer_pipeline::test_overlapping_root(root).unwrap_or_else(|| {
        overlapping_writer(std::env::var("SPACEFOLD_WRITER_MODE").ok().as_deref())
    });
    #[cfg(not(test))]
    let overlap = overlapping_writer(std::env::var("SPACEFOLD_WRITER_MODE").ok().as_deref());
    if overlap {
        writer_pipeline::scan(service, scan_id, root, job, fast, readers, output_slots)
    } else {
        scan_sync_serial_with_queue_capacity(
            service,
            scan_id,
            root,
            job,
            fast,
            readers,
            output_slots,
        )
    }
}
fn scan_sync_serial_with_queue_capacity(
    service: &Service,
    scan_id: &str,
    root: &Path,
    job: &JobRuntime,
    fast: bool,
    readers: usize,
    output_slots: usize,
) -> Result<Scan> {
    assert!(matches!(output_slots, 32 | 64 | 128));
    let started = Instant::now();
    let consumed_limit =
        consumed_limit_override(std::env::var("SPACEFOLD_CONSUMED_LIMIT").ok().as_deref());
    let plan = ReaderPlan::new(readers, consumed_limit)?;
    let profile_enabled = std::env::var_os("SPACEFOLD_PROFILE").is_some();
    let mut profile = ScanProfile {
        enabled: profile_enabled,
        reader_wait_probe: profile_enabled.then(|| std::sync::Arc::new(ReaderWaitProbe::default())),
        consumed_limit,
        directory_readers: readers,
        directory_in_flight_limit: plan.in_flight,
        writer_mode: "serial",
        stored_path_codec: crate::stored_path::Mode::from_environment(),
        metadata_codec: crate::stored_metadata::Mode::from_environment(),
        file_path_storage: crate::stored_path::FileStorage::from_environment(),
        directory_write_mode: crate::repository::DirectoryWriteMode::from_override(
            std::env::var("SPACEFOLD_DIRECTORY_WRITE_FUSION")
                .ok()
                .as_deref(),
        ),
        checkpoint_mode: "off",
        output_slots,
        // crossbeam's bounded slot holds one atomic stamp and the message,
        // with at most one message alignment unit of padding per slot.
        output_slot_storage_upper_bound_bytes: output_slots
            * (std::mem::size_of::<ReaderMessage>()
                + std::mem::size_of::<usize>()
                + std::mem::align_of::<ReaderMessage>()),
        started: Some(started),
        ..Default::default()
    };
    let mut c = service.repo.connect()?;
    // This connection's only TEMP table is a bounded set of <=2048 consumed IDs;
    // durable nodes/work queues retain the repository's WAL/NORMAL policy.
    c.execute_batch(
        "PRAGMA temp_store=MEMORY;CREATE TEMP TABLE consumed_dirs(id INTEGER PRIMARY KEY);",
    )?;
    let rootmeta = platform::metadata(root)?;
    let directory_reader =
        platform::ScanDirectoryReader::new(root, fast).with_profiling(profile.enabled);
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
            NodeInserter::with_codecs(
                &tx,
                false,
                profile.stored_path_codec,
                profile.metadata_codec,
            )?
            .insert(scan_id, 1, None, root, 0, &rootmeta, None)?;
            tx.execute(
                "UPDATE scans SET discovered=discovered+1 WHERE id=?1",
                [scan_id],
            )?;
        }
        tx.commit()?;
    }
    let (work_tx, work_rx) = crossbeam_channel::bounded::<(i64, i64, PathBuf, u32)>(2000);
    let (result_tx, result_rx) = crossbeam_channel::bounded::<MetadataResult>(2000);
    let mut handles = vec![];
    let mut metadata_setup_error = None;
    for index in 0..4 {
        let receiver = work_rx.clone();
        let sender = result_tx.clone();
        match spawn_worker(root, "metadata", index, move || {
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
        }) {
            Ok(handle) => handles.push(handle),
            Err(error) => {
                metadata_setup_error = Some(thread_start_error(error));
                break;
            }
        }
    }
    drop(work_rx);
    drop(result_tx);
    if let Some(error) = metadata_setup_error {
        drop(work_tx);
        drop(result_rx);
        for handle in handles {
            let _ = handle.join();
        }
        if let Err((connection, issue)) = c.close() {
            drop(connection);
            return Err(issue.into());
        }
        return Err(error);
    }
    let mut next_id = 2i64;
    let mut last_emit = Instant::now();
    let mut failed: Option<AppError> = None;
    let mut pending = 0usize;
    let mut results = Vec::<MetadataResult>::with_capacity(BATCH_ENTRIES);
    let mut directory_updates = Vec::<DirectoryMetadataUpdate>::with_capacity(256);
    let mut batch_path_bytes = 0usize;
    let mut consumed = 0usize;
    let mut batch_started = Instant::now();
    let auxiliary_budget = std::sync::Arc::new(ReaderBudget {
        limit: AUXILIARY_MESSAGE_BYTES,
        ..Default::default()
    });
    let pipeline_result = std::thread::scope(|scope| {
        let (request_tx, request_rx) = crossbeam_channel::bounded(DIRECTORY_PREFETCH);
        let (directory_tx, directory_rx) = crossbeam_channel::bounded(output_slots);
        let enabled = profile.enabled;
        let budget = std::sync::Arc::new(ReaderBudget {
            observation: profile.reader_wait_probe.clone(),
            ..Default::default()
        });
        let reader_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut readers = Vec::new();
        let mut setup_error = readers
            .try_reserve_exact(profile.directory_readers)
            .err()
            .map(|_| AppError::new("RESOURCE_LIMIT", "扫描线程句柄超过内存预算"));
        for index in 0..profile.directory_readers {
            if setup_error.is_some() {
                break;
            }
            let requests = request_rx.clone();
            let output = directory_tx.clone();
            let reader_budget = budget.clone();
            let auxiliary_budget = auxiliary_budget.clone();
            let stop = reader_stop.clone();
            let directory_reader = directory_reader.clone();
            let path_codec = profile.stored_path_codec;
            let metadata_codec = profile.metadata_codec;
            let file_path_storage = profile.file_path_storage;
            match spawn_scoped_worker(scope, root, "reader", index, move || -> Result<()> {
                let fault_output = output.clone();
                let output = ReaderOutput {
                    sender: output,
                    observation: reader_budget.observation.clone(),
                    auxiliary_budget: auxiliary_budget.clone(),
                    cancel: &job.cancel,
                    path_codec,
                    metadata_codec,
                    file_path_storage,
                };
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    read_directories(
                        requests,
                        output,
                        &job.cancel,
                        &stop,
                        enabled,
                        reader_budget.clone(),
                        directory_reader,
                    )
                }));
                if outcome.is_err() {
                    stop.store(true, Ordering::Release);
                    reader_budget.stop();
                    auxiliary_budget.stop();
                    let error = AppError::new("WORKER_FAILED", "枚举线程异常退出");
                    let _ = fault_output.send(ReaderMessage {
                        message: DirectoryMessage::Fatal(error.clone()),
                        auxiliary_permit: None,
                    });
                    return Err(error);
                }
                Ok(())
            }) {
                Ok(reader) => readers.push(reader),
                Err(error) => {
                    setup_error = Some(thread_start_error(error));
                    break;
                }
            }
        }
        drop(request_rx);
        drop(directory_tx);
        let result = if let Some(error) = setup_error {
            Err(error)
        } else {
            (|| -> Result<()> {
                let mut in_flight = 0usize;
                let mut refill = true;
                let mut queue_cursor = DirectoryQueueCursor::default();
                'directories: loop {
                    if job.cancel.load(Ordering::Acquire) {
                        break;
                    }
                    if reader_stop.load(Ordering::Acquire) {
                        return Err(AppError::new("WORKER_FAILED", "枚举线程异常退出"));
                    }
                    if consumed >= consumed_limit
                        || consumed > 0 && batch_started.elapsed() >= Duration::from_millis(100)
                    {
                        if profile.enabled {
                            profile.flush_consumed_limit += usize::from(consumed >= consumed_limit);
                            profile.flush_time_limit +=
                                usize::from(batch_started.elapsed() >= Duration::from_millis(100));
                        }
                        flush_batch(
                            service,
                            &mut c,
                            scan_id,
                            pending,
                            &result_rx,
                            &mut results,
                            &mut directory_updates,
                            &mut profile,
                        )?;
                        pending = 0;
                        batch_path_bytes = 0;
                        consumed = 0;
                        queue_cursor.reset_after_flush();
                        batch_started = Instant::now();
                        refill = true;
                    }
                    if refill && in_flight < plan.in_flight {
                        let started = Instant::now();
                        let limit = (plan.in_flight - in_flight).min(consumed_limit - consumed);
                        let mark_started = profile.enabled.then(Instant::now);
                        let mut mark_consumed =
                            c.prepare_cached("INSERT INTO consumed_dirs(id) VALUES(?1)")?;
                        if let Some(mark_started) = mark_started {
                            profile.directory_queue_mark_ms += milliseconds(mark_started);
                        }
                        for _ in 0..limit {
                            if queue_cursor.needs_refill() {
                                let read_started = profile.enabled.then(Instant::now);
                                let read = queue_cursor.refill(&c, scan_id)?;
                                if let Some(read_started) = read_started {
                                    profile.directory_queue_read_ms += milliseconds(read_started);
                                    profile.directory_queue_queries += 1;
                                    profile.directory_queue_read_rows += read;
                                    profile.directory_queue_peak_cached_entries = profile
                                        .directory_queue_peak_cached_entries
                                        .max(queue_cursor.cache.len());
                                    profile.directory_queue_peak_cached_bytes = profile
                                        .directory_queue_peak_cached_bytes
                                        .max(queue_cursor.owned_bytes());
                                }
                            }
                            if queue_cursor.cache.is_empty() {
                                break;
                            }
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
                            let mark_started = profile.enabled.then(Instant::now);
                            let (parent, path, depth, expected_identity, metadata_deferred) =
                                queue_cursor.consume(&mut mark_consumed)?;
                            if let Some(mark_started) = mark_started {
                                profile.directory_queue_mark_ms += milliseconds(mark_started);
                            }
                            let decode_started = profile.enabled.then(Instant::now);
                            let path = crate::stored_path::decode(path)
                                .map_err(crate::stored_path::app_error)?;
                            if let Some(decode_started) = decode_started {
                                profile.directory_queue_decode_ms += milliseconds(decode_started);
                            }
                            consumed += 1;
                            if depth >= 4096 {
                                let error_started = profile.enabled.then(Instant::now);
                                record_error(
                                    service,
                                    &c,
                                    scan_id,
                                    parent,
                                    &path,
                                    "enumerate",
                                    AppError::new("DEPTH_LIMIT", "目录深度达到 4096，未继续读取"),
                                )?;
                                if let Some(error_started) = error_started {
                                    profile.directory_queue_error_ms += milliseconds(error_started);
                                }
                            } else {
                                let send_started = profile.enabled.then(Instant::now);
                                let sent = request_tx.send((
                                    parent,
                                    path,
                                    depth,
                                    expected_identity,
                                    metadata_deferred,
                                ));
                                if let Some(send_started) = send_started {
                                    profile.directory_queue_send_ms += milliseconds(send_started);
                                }
                                if sent.is_err() {
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
                        if pending > 0
                            || !results.is_empty()
                            || consumed > 0
                            || !directory_updates.is_empty()
                        {
                            if profile.enabled {
                                profile.flush_drain += 1;
                            }
                            flush_batch(
                                service,
                                &mut c,
                                scan_id,
                                pending,
                                &result_rx,
                                &mut results,
                                &mut directory_updates,
                                &mut profile,
                            )?;
                            pending = 0;
                            batch_path_bytes = 0;
                            consumed = 0;
                            queue_cursor.reset_after_flush();
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
                    let ReaderMessage {
                        message,
                        mut auxiliary_permit,
                    } = message;
                    match message {
                        DirectoryMessage::Metadata(update) => {
                            batch_path_bytes +=
                                update.bytes() + std::mem::size_of::<DirectoryMetadataUpdate>();
                            profile.peak_batch_path_bytes =
                                profile.peak_batch_path_bytes.max(batch_path_bytes);
                            directory_updates.push(update);
                            drop(auxiliary_permit.take());
                            // Existing collection time and owned-byte limits also
                            // bound the independently completed directory metadata.
                            if batch_path_bytes >= BATCH_PATH_BYTES
                                || directory_updates.len() >= 2048
                                || batch_started.elapsed() >= Duration::from_millis(100)
                            {
                                if profile.enabled {
                                    profile.flush_path_limit +=
                                        usize::from(batch_path_bytes >= BATCH_PATH_BYTES);
                                    profile.flush_metadata_limit +=
                                        usize::from(directory_updates.len() >= 2048);
                                    profile.flush_time_limit += usize::from(
                                        batch_started.elapsed() >= Duration::from_millis(100),
                                    );
                                }
                                flush_batch(
                                    service,
                                    &mut c,
                                    scan_id,
                                    pending,
                                    &result_rx,
                                    &mut results,
                                    &mut directory_updates,
                                    &mut profile,
                                )?;
                                pending = 0;
                                batch_path_bytes = 0;
                                consumed = 0;
                                queue_cursor.reset_after_flush();
                                batch_started = Instant::now();
                                refill = true;
                            }
                        }
                        DirectoryMessage::Entries {
                            parent,
                            path,
                            depth,
                            entries,
                            permit,
                        } => {
                            drop(path);
                            drop(auxiliary_permit.take());
                            for (entry, encoded) in entries {
                                if job.cancel.load(Ordering::Acquire) {
                                    break 'directories;
                                }
                                let ordinary_file = entry
                                    .scan_file_metadata()
                                    .or_else(|| entry.scan_directory_metadata())
                                    .is_some();
                                let p = entry.path;
                                if p.starts_with(&service.repo.data_dir) || service.is_work_file(&p)
                                {
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
                                if let Some(metadata) = entry.hint.filter(|_| ordinary_file) {
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
                                    if profile.enabled {
                                        profile.flush_entries_limit +=
                                            usize::from(results.len() + pending >= BATCH_ENTRIES);
                                        profile.flush_metadata_limit +=
                                            usize::from(pending >= 2000);
                                        profile.flush_path_limit +=
                                            usize::from(batch_path_bytes >= BATCH_PATH_BYTES);
                                        profile.flush_time_limit += usize::from(
                                            batch_started.elapsed() >= Duration::from_millis(100),
                                        );
                                    }
                                    flush_batch(
                                        service,
                                        &mut c,
                                        scan_id,
                                        pending,
                                        &result_rx,
                                        &mut results,
                                        &mut directory_updates,
                                        &mut profile,
                                    )?;
                                    pending = 0;
                                    batch_path_bytes = 0;
                                    consumed = 0;
                                    queue_cursor.reset_after_flush();
                                    batch_started = Instant::now();
                                    refill = true;
                                }
                            }
                            drop(permit);
                        }
                        DirectoryMessage::Error {
                            parent,
                            path,
                            stage,
                            error,
                        } => record_error(service, &c, scan_id, parent, &path, stage, error)?,
                        DirectoryMessage::Done(_id, timings) => {
                            in_flight -= 1;
                            profile.enumerate_ms += timings.enumerate_ms;
                            profile.directory_check_ms += timings.directory_check_ms;
                            profile.encode_ms += timings.encode_ms;
                            refill = true;
                        }
                        DirectoryMessage::Fatal(error) => return Err(error),
                    }
                    // Metadata/entries now belong to the existing bounded writer
                    // batch, or the error has been saved. Transfer their ownership
                    // before returning this separate queue-only auxiliary charge.
                    drop(auxiliary_permit);
                    if last_emit.elapsed() >= Duration::from_millis(100) {
                        emit_scan(service, scan_id, job, "running", "读取目录与元数据")?;
                        last_emit = Instant::now();
                    }
                }
                Ok(())
            })()
        };
        // Dropping both channels wakes a reader blocked by backpressure, on
        // cancellation and on errors. Join it before flushing/finalizing state.
        drop(request_tx);
        drop(directory_rx);
        reader_stop.store(true, Ordering::Release);
        budget.stop();
        auxiliary_budget.stop();
        #[cfg(test)]
        {
            let hook = DEFERRED_DIRECTORY_HOOK
                .lock()
                .unwrap()
                .as_ref()
                .filter(|h| h.0.parent() == Some(root))
                .cloned();
            if let Some((_, _, _, stopped)) = hook {
                let _ = stopped.try_send(());
            }
        }
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
    if profile.enabled {
        profile.directory_api = directory_reader.api_profile();
    }
    // Cancellation keeps every already-discovered metadata result in the partial
    // snapshot. Errors keep the preceding committed revision and fail the job.
    if failed.is_none() && (pending > 0 || !results.is_empty() || !directory_updates.is_empty()) {
        if profile.enabled {
            profile.flush_final += 1;
        }
        if let Err(error) = flush_batch(
            service,
            &mut c,
            scan_id,
            pending,
            &result_rx,
            &mut results,
            &mut directory_updates,
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
        if let Err((connection, issue)) = c.close() {
            drop(connection);
            return Err(issue.into());
        }
        return Err(error);
    }
    let scan = service.repo.scan(scan_id)?;
    if !job.cancel.load(Ordering::Acquire) {
        crate::repository::check_directory_metadata_complete(&c, scan_id)?;
    }
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
        let auxiliary = auxiliary_budget.state.lock().unwrap();
        profile.auxiliary_message_peak_bytes = auxiliary.2;
        profile.auxiliary_message_budget_waits = auxiliary.3;
        if let Some(probe) = &profile.reader_wait_probe {
            profile.reader_backpressure = probe.snapshot();
        }
        for (op, output) in [
            (
                rusqlite::ffi::SQLITE_DBSTATUS_CACHE_WRITE,
                &mut profile.cache_write_pages,
            ),
            (
                rusqlite::ffi::SQLITE_DBSTATUS_CACHE_HIT,
                &mut profile.cache_hit_pages,
            ),
            (
                rusqlite::ffi::SQLITE_DBSTATUS_CACHE_MISS,
                &mut profile.cache_miss_pages,
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
    }
    // Terminal means this scanner's SQLite resources have also been closed.
    // Otherwise a caller exiting on the final event could kill the connection's
    // natural last-close checkpoint while the complete data still lives in WAL.
    let close_started = Instant::now();
    if let Err((connection, error)) = c.close() {
        drop(connection);
        return Err(error.into());
    }
    profile.close_ms = milliseconds(close_started);
    if profile.enabled {
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
    let tx = c.unchecked_transaction()?;
    Repository::error_conn(
        &tx,
        scan_id,
        ScanError {
            node_id: Some(id.to_string()),
            display_path: platform::display(path),
            stage: stage.into(),
            error,
        },
    )?;
    tx.execute(
        "DELETE FROM directory_metadata_pending WHERE scan_id=?1 AND id=?2",
        params![scan_id, id],
    )?;
    tx.commit()?;
    Ok(())
}
#[allow(clippy::too_many_arguments)] // Both result buffers share this one discovery/revision transaction.
fn flush_batch(
    service: &Service,
    c: &mut rusqlite::Connection,
    scan_id: &str,
    count: usize,
    receiver: &crossbeam_channel::Receiver<MetadataResult>,
    results: &mut Vec<MetadataResult>,
    directory_updates: &mut Vec<DirectoryMetadataUpdate>,
    profile: &mut ScanProfile,
) -> Result<()> {
    flush_batch_with_errors(
        service,
        c,
        scan_id,
        count,
        receiver,
        results,
        directory_updates,
        &mut Vec::new(),
        profile,
    )
}
#[allow(clippy::too_many_arguments)] // Both packet buffers and errors share one revision transaction.
fn flush_batch_with_errors(
    service: &Service,
    c: &mut rusqlite::Connection,
    scan_id: &str,
    count: usize,
    receiver: &crossbeam_channel::Receiver<MetadataResult>,
    results: &mut Vec<MetadataResult>,
    directory_updates: &mut Vec<DirectoryMetadataUpdate>,
    errors: &mut Vec<writer_pipeline::PendingScanError>,
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
    let flush_window = profile
        .reader_wait_probe
        .as_ref()
        .map(|probe| probe.begin(ReaderWaitRole::Flush));
    let wait_started = Instant::now();
    let _writer = service.writer.lock().unwrap();
    profile.writer_wait_ms += milliseconds(wait_started);
    // The guard stays outside the unwind boundary. A panic rolls back the
    // transaction before the lock is normally released, without poisoning the
    // job's failure-finalization path.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let tx = c.transaction()?;
        let insert_started = Instant::now();
        let prepared = if profile.staged_insert_mode.enabled() {
            Some(staging::PreparedBatch::prepare(results, directory_updates)?)
        } else {
            None
        };
        if let Some(prepared) = &prepared {
            profile.staged_metadata_inserts += prepared.absorbed_metadata;
            profile.staged_internal_edges += prepared.absorbed_edges;
            profile.staged_counter_storage_upper_bound_bytes = profile
                .staged_counter_storage_upper_bound_bytes
                .max(prepared.totals.capacity() * std::mem::size_of::<[i64; 6]>());
        }
        let mut inserter = NodeInserter::with_codecs(
            &tx,
            profile.enabled,
            profile.stored_path_codec,
            profile.metadata_codec,
        )?;
        let mut aggregates = std::collections::BTreeMap::<i64, [i64; 6]>::new();
        // Enumeration assigns increasing stable IDs. Sort the exceptional worker
        // results back into that order so primary/index inserts remain sequential.
        results.sort_unstable_by_key(|entry| entry.0);
        for (row_index, (id, parent, path, depth, result, encoded)) in results.drain(..).enumerate()
        {
            let mut own_only = [0; 6];
            let totals = if prepared.is_some() {
                &mut own_only
            } else {
                aggregates.entry(parent).or_default()
            };
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
                    if let Some(prepared) = &prepared {
                        inserter.insert_prepared(
                            scan_id,
                            id,
                            Some(parent),
                            &path,
                            depth,
                            &m,
                            error.as_ref(),
                            encoded.as_ref(),
                            prepared.totals[row_index],
                        )?;
                    } else if let Some(encoded) = encoded {
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
                        inserter.insert(
                            scan_id,
                            id,
                            Some(parent),
                            &path,
                            depth,
                            &m,
                            error.as_ref(),
                        )?;
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
                    if let Some(prepared) = &prepared {
                        inserter.insert_prepared(
                            scan_id,
                            id,
                            Some(parent),
                            &path,
                            depth,
                            &m,
                            Some(&error),
                            None,
                            prepared.totals[row_index],
                        )?;
                    } else {
                        inserter.insert(
                            scan_id,
                            id,
                            Some(parent),
                            &path,
                            depth,
                            &m,
                            Some(&error),
                        )?;
                    }
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
        let discovered_extra = prepared
            .as_ref()
            .map_or(0, |prepared| prepared.discovered_extra);
        if let Some(prepared) = prepared {
            aggregates = prepared.frontier;
        }
        if let Some(timings) = &inserter.timings {
            profile.encode_ms += timings.encode_ms;
            profile.sql_insert_ms += timings.sql_ms;
            profile.pending_insert_ms += timings.pending_ms;
        }
        drop(inserter);
        let row_insert_ms = milliseconds(insert_started);
        let metadata_started = profile.enabled.then(Instant::now);
        let mut sql_probe = profile
            .enabled
            .then(crate::repository::DirectoryWriteSqlProbe::default);
        if profile.enabled {
            profile.directory_metadata_updates += directory_updates.len();
        }
        let fusion = if profile.directory_write_mode.fused() {
            crate::repository::prepare_directory_fusion_with_codec(
                &tx,
                scan_id,
                directory_updates,
                &aggregates,
                !errors.is_empty(),
                profile.metadata_codec,
                sql_probe.as_mut(),
            )?
        } else {
            None
        };
        let aggregation = if let Some(plan) = fusion {
            let result = plan.apply(&tx, scan_id, sql_probe.as_mut())?;
            // Eligibility excludes error-bearing batches, so this remains an
            // empty no-op and error order is unchanged on every fallback.
            writer_pipeline::persist_errors(&tx, scan_id, errors)?;
            profile.directory_metadata_apply_ms += result.metadata_ms;
            profile.insert_ms += row_insert_ms + result.metadata_ms;
            profile.aggregate_ms += result.aggregate_ms;
            result.aggregate
        } else {
            for update in directory_updates.iter() {
                crate::repository::apply_directory_metadata_with_codec(
                    &tx,
                    scan_id,
                    update,
                    profile.metadata_codec,
                    sql_probe.as_mut(),
                )?;
            }
            writer_pipeline::persist_errors(&tx, scan_id, errors)?;
            let metadata_ms = metadata_started.map_or(0.0, milliseconds);
            profile.directory_metadata_apply_ms += metadata_ms;
            profile.insert_ms += row_insert_ms + metadata_ms;
            let aggregate_started = Instant::now();
            let result = crate::repository::aggregate_many_observed(
                &tx,
                scan_id,
                &aggregates,
                sql_probe.as_mut(),
            )?;
            profile.aggregate_ms += milliseconds(aggregate_started);
            result
        };
        directory_updates.clear();
        if let Some(probe) = sql_probe {
            profile.directory_write_sql.merge(probe.finish());
        }
        if discovered_extra != 0 {
            let discovered: i64 = tx.query_row(
                "SELECT discovered FROM scans WHERE id=?1",
                [scan_id],
                |row| row.get(0),
            )?;
            let discovered = crate::repository::checked_add(discovered, discovered_extra)?;
            tx.execute(
                "UPDATE scans SET discovered=?2 WHERE id=?1",
                params![scan_id, discovered],
            )?;
        }
        profile.aggregate_direct_parents += aggregation.direct_parents;
        profile.aggregate_ancestor_rows += aggregation.ancestor_rows;
        profile.aggregate_fallback_batches += usize::from(!aggregation.coalesced);
        tx.execute(
            "UPDATE scans SET revision=revision+1 WHERE id=?1",
            [scan_id],
        )?;
        tx.execute(
            "DELETE FROM pending_dirs WHERE scan_id=?1 AND id IN consumed_dirs",
            [scan_id],
        )?;
        tx.execute("DELETE FROM consumed_dirs", [])?;
        #[cfg(test)]
        writer_pipeline::test_before_commit(scan_id, &service.repo.data_dir);
        let commit_started = Instant::now();
        let commit_window = profile
            .reader_wait_probe
            .as_ref()
            .map(|probe| probe.begin(ReaderWaitRole::Commit));
        tx.commit()?;
        drop(commit_window);
        drop(flush_window);
        let commit_ms = milliseconds(commit_started);
        profile.commit_ms += commit_ms;
        if profile.enabled && !profile.defer_commit_observation {
            profile.writer_observation.on_commit(c, scan_id, commit_ms);
        }
        profile.batches += 1;
        profile.max_flush_ms = profile.max_flush_ms.max(milliseconds(flush_started));
        if profile.first_commit_ms.is_none() && profile.peak_batch_entries > 0 {
            profile.first_commit_ms = profile.started.map(milliseconds);
        }
        Ok(())
    }))
    .unwrap_or_else(|_| {
        Err(AppError::new(
            "WORKER_FAILED",
            "扫描写入事务异常，已回滚当前批次",
        ))
    })
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
    tx.execute_batch("CREATE TEMP TABLE refresh_ids(old_id INTEGER PRIMARY KEY,new_id INTEGER); CREATE TEMP TABLE refresh_old(path BLOB PRIMARY KEY,id INTEGER,identity TEXT,kind TEXT,logical BLOB,modified TEXT,reparse_tag INTEGER); CREATE TEMP TABLE refreshed_nodes(id INTEGER PRIMARY KEY); CREATE TEMP TABLE refresh_paths(scan_id TEXT NOT NULL,id INTEGER NOT NULL,path BLOB NOT NULL,identity TEXT,kind TEXT,logical BLOB,modified TEXT,reparse_tag INTEGER,payload,PRIMARY KEY(scan_id,id)) WITHOUT ROWID;")?;
    tx.execute("WITH RECURSIVE subtree(id) AS (SELECT ?2 UNION ALL SELECT n.id FROM subtree s CROSS JOIN nodes n ON n.scan_id=?1 AND n.parent_id=s.id) INSERT INTO refreshed_nodes SELECT id FROM subtree",params![target_scan,parse_id(scope_id)?])?;
    tx.execute(
        "DELETE FROM directory_metadata_pending WHERE scan_id=?1 AND id IN refreshed_nodes",
        [target_scan],
    )?;
    // Persistent paths may mix legacy UTF16 and WTF8. Build exact legacy keys
    // one row at a time; path equality never replaces the identity checks below.
    {
        let mut paths = tx.prepare(&format!("SELECT {},scan_id FROM nodes WHERE (scan_id=?1 AND id IN refreshed_nodes) OR scan_id=?2", crate::repository::NODE_COLUMNS))?;
        let mut rows = paths.query(params![target_scan, staging_scan])?;
        let mut insert = tx.prepare("INSERT INTO refresh_paths(scan_id,id,path,identity,kind,logical,modified,reparse_tag,payload) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)")?;
        while let Some(row) = rows.next()? {
            let stored = crate::repository::read_node(row).map_err(|error| match error {
                rusqlite::Error::FromSqlConversionFailure(2, _, cause) => {
                    AppError::new("STORED_PATH_ENCODING", cause.to_string())
                }
                error => AppError::from(error),
            })?;
            let scan: String = row.get(20)?;
            let payload = if scan == staging_scan {
                let value = row.get_ref(3)?;
                Some(
                    crate::stored_metadata::Payload::from_ref(value)
                        .map_err(crate::stored_metadata::app_error)?,
                )
            } else {
                None
            };
            insert.execute(params![
                scan,
                row.get::<_, i64>(0)?,
                platform::encode_path(&stored.path),
                stored.fingerprint.identity,
                row.get::<_, String>(15)?,
                stored.fingerprint.logical.to_le_bytes().as_slice(),
                row.get::<_, Option<String>>(17)?,
                stored.fingerprint.reparse_tag,
                payload,
            ])?;
        }
    }
    tx.execute("INSERT INTO refresh_old SELECT path,id,identity,kind,logical,modified,reparse_tag FROM refresh_paths WHERE scan_id=?1",[target_scan])?;
    let max: i64 = tx.query_row(
        "SELECT COALESCE(MAX(id),0) FROM nodes WHERE scan_id=?1",
        [target_scan],
        |r| r.get(0),
    )?;
    tx.execute("INSERT INTO refresh_ids(old_id,new_id) SELECT n.id,CASE WHEN n.id=1 THEN ?2 ELSE COALESCE((SELECT old.id FROM refresh_old old WHERE old.path=k.path AND (old.identity=k.identity OR ((old.identity IS NULL OR k.identity IS NULL) AND old.kind=k.kind AND old.logical=k.logical AND old.modified IS k.modified AND old.reparse_tag=k.reparse_tag))),n.id+?3) END FROM nodes n JOIN refresh_paths k ON k.scan_id=n.scan_id AND k.id=n.id WHERE n.scan_id=?1",params![staging_scan,parse_id(scope_id)?,max])?;
    tx.execute("DELETE FROM scan_errors WHERE scan_id=?1 AND CAST(json_extract(payload,'$.nodeId') AS INTEGER) IN refreshed_nodes",[target_scan])?;
    tx.execute("WITH RECURSIVE subtree(id) AS (SELECT id FROM nodes WHERE scan_id=?1 AND parent_id=?2 UNION ALL SELECT n.id FROM subtree s CROSS JOIN nodes n ON n.scan_id=?1 AND n.parent_id=s.id) DELETE FROM nodes WHERE scan_id=?1 AND id IN subtree",params![target_scan,parse_id(scope_id)?])?;
    // Unchanged paths and file identities retain IDs; replaced identities receive fresh IDs.
    let root_payload: crate::stored_metadata::Payload = tx.query_row(
        "SELECT payload FROM refresh_paths WHERE scan_id=?1 AND id=1",
        [staging_scan],
        |r| {
            let value = r.get_ref(0)?;
            crate::stored_metadata::Payload::from_ref(value)
                .map_err(|e| crate::stored_metadata::sql_error(0, value.data_type(), e))
        },
    )?;
    tx.execute("UPDATE nodes SET payload=?3,logical_total=?4,allocated_total=?5,allocated_unknown=?6,file_count=?7,dir_count=?8,children_count=?9,incomplete=?10,stale=0,error_code=?11,modified=?12,kind=?13,extension=?14 WHERE scan_id=?1 AND id=?2",params![target_scan,parse_id(scope_id)?,root_payload,parse_bytes(&new.node.logical_total_bytes)?,parse_bytes(&new.node.allocated_known_total_bytes)?,parse_bytes(&new.node.allocated_unknown_count)?,parse_bytes(&new.node.file_count)?,parse_bytes(&new.node.directory_count)?,parse_bytes(&new.node.children_count)?,new.node.incomplete,new.node.error_code,new.node.modified_at,new.node.kind,new.node.extension])?;
    tx.execute("INSERT INTO nodes(scan_id,id,parent_id,path,payload,name,kind,extension,depth,modified,logical_total,allocated_total,allocated_unknown,file_count,dir_count,children_count,incomplete,stale,error_code,search_path) SELECT ?1,m.new_id,p.new_id,n.path,k.payload,n.name,n.kind,n.extension,n.depth+?3,n.modified,n.logical_total,n.allocated_total,n.allocated_unknown,n.file_count,n.dir_count,n.children_count,CASE WHEN n.incomplete=2 THEN (SELECT status NOT IN('completed','completed_with_errors') FROM scans WHERE id=n.scan_id) ELSE n.incomplete END,0,n.error_code,n.search_path FROM nodes n JOIN refresh_paths k ON k.scan_id=n.scan_id AND k.id=n.id JOIN refresh_ids m ON n.id=m.old_id JOIN refresh_ids p ON n.parent_id=p.old_id WHERE n.scan_id=?2 AND n.id<>1",params![target_scan,staging_scan,old.node.depth])?;
    for ancestor in ancestors.into_iter().rev() {
        let delta = parse_bytes(&new.node.logical_total_bytes)?
            - parse_bytes(&old.node.logical_total_bytes)?;
        let adelta = parse_bytes(&new.node.allocated_known_total_bytes)?
            - parse_bytes(&old.node.allocated_known_total_bytes)?;
        checked_aggregate(&ancestor.node, delta, adelta)?;
        tx.execute("UPDATE nodes SET logical_total=logical_total+?3,allocated_total=allocated_total+?4,allocated_unknown=allocated_unknown+?5,file_count=file_count+?6,dir_count=dir_count+?7 WHERE scan_id=?1 AND id=?2",params![target_scan,parse_id(&ancestor.node.node_id)?,delta,adelta,parse_bytes(&new.node.allocated_unknown_count)?-parse_bytes(&old.node.allocated_unknown_count)?,parse_bytes(&new.node.file_count)?-parse_bytes(&old.node.file_count)?,parse_bytes(&new.node.directory_count)?-parse_bytes(&old.node.directory_count)?+i64::from(new.node.kind=="directory")-i64::from(old.node.kind=="directory")])?;
        tx.execute("UPDATE nodes SET incomplete=CASE WHEN error_code IS NULL AND NOT EXISTS(SELECT 1 FROM nodes child WHERE child.scan_id=?1 AND child.parent_id=?2 AND (child.incomplete=1 OR (child.incomplete=2 AND (SELECT status FROM scans WHERE id=?1) NOT IN('completed','completed_with_errors')))) THEN 0 ELSE 1 END WHERE scan_id=?1 AND id=?2",params![target_scan,parse_id(&ancestor.node.node_id)?])?;
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
    // These three deterministic tests inject the same process-global reader
    // hooks. Serialize only their instrumentation, not production scan access.
    static READER_CONTROL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    #[test]
    fn dynamic_reader_plan_gives_sixty_four_readers_committed_work() {
        let _control = READER_CONTROL_TEST_LOCK.lock().unwrap();
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        for index in 0..64 {
            let directory = source.join(format!("child-{index}"));
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("file"), b"abc").unwrap();
        }
        let root = platform::absolute(&source).unwrap();
        let service = Service::new(&fixture.path().join("data")).unwrap();
        let id = service.repo.new_scan_id().unwrap();
        service.repo.insert_scan(&id, &root, false).unwrap();
        let job = JobRuntime::new(JobEvent::new(&id, &id, "scan", 1), None);
        let (entered_tx, entered_rx) = crossbeam_channel::bounded(64);
        let (release_tx, release_rx) = crossbeam_channel::unbounded();
        *PARALLEL_WORK_HOOK.lock().unwrap() = Some(ParallelWorkHook {
            root: root.clone(),
            entered: entered_tx,
            release: release_rx,
        });
        struct Clear;
        impl Drop for Clear {
            fn drop(&mut self) {
                *PARALLEL_WORK_HOOK.lock().unwrap() = None;
            }
        }
        let _clear = Clear;
        std::thread::scope(|scope| {
            let scan =
                scope.spawn(|| writer_pipeline::scan(&service, &id, &root, &job, true, 64, 32));
            let all_entered = (0..64)
                .try_for_each(|_| entered_rx.recv_timeout(Duration::from_secs(5)).map(|_| ()));
            let committed = service.repo.scan(&id).unwrap();
            // Release even after a failed barrier so a test assertion cannot
            // strand readers during the scope's mandatory joins.
            for _ in 0..64 {
                release_tx.send(()).unwrap();
            }
            let completed = scan.join().unwrap().unwrap();
            all_entered.unwrap();
            assert_eq!(committed.discovered_count, "65");
            assert_eq!(completed.discovered_count, "129");
            assert_eq!(completed.logical_total_bytes, "192");
            assert_eq!(completed.status, "completed");
            assert!(job.terminal());
            let c = service.repo.connect().unwrap();
            crate::repository::check_directory_metadata_complete(&c, &id).unwrap();
        });
    }

    #[test]
    fn partial_thread_start_failures_join_workers_close_and_allow_another_scan() {
        let _control = READER_CONTROL_TEST_LOCK.lock().unwrap();
        let _start_control = START_FAULT_TEST_LOCK.lock().unwrap();
        for (role, index, overlap, expected_started) in [
            ("writer", 0, true, 0),
            ("reader", 3, true, 4),
            ("metadata", 2, true, 7),
            ("reader", 3, false, 7),
            ("metadata", 2, false, 2),
        ] {
            let fixture = tempfile::tempdir().unwrap();
            let source = fixture.path().join("source");
            fs::create_dir(&source).unwrap();
            fs::write(source.join("preserved"), b"own data").unwrap();
            let root = platform::absolute(&source).unwrap();
            let service = Service::new(&fixture.path().join("data")).unwrap();
            let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            *START_FAULT.lock().unwrap() = Some(StartFault {
                root: root.clone(),
                role,
                index,
                active: active.clone(),
                started: started.clone(),
            });
            struct Clear;
            impl Drop for Clear {
                fn drop(&mut self) {
                    *START_FAULT.lock().unwrap() = None;
                }
            }
            let _clear = Clear;
            let _mode = writer_pipeline::test_writer_mode(&root, overlap);
            let scan = service.start_scan(root.clone(), None).unwrap();
            let event = terminal(&service, &scan.scan_id);
            assert_eq!(event.status, "failed", "{role}/{overlap}: {event:?}");
            assert_eq!(event.error.unwrap().code, "WORKER_START_FAILED");
            assert_eq!(active.load(Ordering::Acquire), 0);
            assert_eq!(started.load(Ordering::Acquire), expected_started);
            assert_eq!(service.repo.scan(&scan.scan_id).unwrap().status, "failed");
            assert_eq!(fs::read(source.join("preserved")).unwrap(), b"own data");
            assert_eq!(
                fs::metadata(service.repo.path.with_extension("sqlite3-wal"))
                    .map_or(0, |m| m.len()),
                0
            );
            *START_FAULT.lock().unwrap() = None;
            let recovered = service.start_scan(root.clone(), None).unwrap();
            assert_eq!(terminal(&service, &recovered.scan_id).status, "completed");
            assert_eq!(
                service
                    .repo
                    .scan(&recovered.scan_id)
                    .unwrap()
                    .logical_total_bytes,
                "8"
            );
        }
    }
    #[test]
    fn parallel_readers_cancel_with_idle_peers_and_join_after_a_panic() {
        let _control = READER_CONTROL_TEST_LOCK.lock().unwrap();
        for (readers, overlap) in [
            (4, false),
            (8, false),
            (8, true),
            (16, false),
            (16, true),
            (1, true),
            (3, true),
            (24, true),
            (64, true),
        ] {
            let fixture = tempfile::tempdir().unwrap();
            let source = fixture.path().join("parallel-reader-source");
            fs::create_dir(&source).unwrap();
            fs::write(source.join("unread.txt"), b"abc").unwrap();
            let root = platform::absolute(&source).unwrap();
            let _mode = writer_pipeline::test_writer_mode(&root, overlap);
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
            struct Clear;
            impl Drop for Clear {
                fn drop(&mut self) {
                    *READER_TEST_HOOK.lock().unwrap() = None;
                    *READER_PANIC_PATH.lock().unwrap() = None;
                }
            }
            let _clear = Clear;
            let id = service.repo.new_scan_id().unwrap();
            service.repo.insert_scan(&id, &root, false).unwrap();
            let job = Arc::new(JobRuntime::new(JobEvent::new(&id, &id, "scan", 1), None));
            std::thread::scope(|scope| {
                let worker = scope.spawn(|| {
                    if overlap {
                        writer_pipeline::scan(
                            &service,
                            &id,
                            &root,
                            &job,
                            true,
                            readers,
                            READER_QUEUE_CHUNKS,
                        )
                    } else {
                        scan_sync_with_options(&service, &id, &root, &job, true, readers)
                    }
                });
                entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                waiting_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                let started = Instant::now();
                job.cancel.store(true, Ordering::Release);
                release_tx.send(()).unwrap();
                let scan = worker.join().unwrap().unwrap();
                assert_eq!(scan.status, "cancelled");
                assert!(job.terminal());
                assert_eq!(job.snapshot().status, "cancelled");
                assert_eq!(scan.discovered_count, "1");
                assert!(started.elapsed() < Duration::from_secs(1));
            });
            *READER_TEST_HOOK.lock().unwrap() = None;
            *READER_PANIC_PATH.lock().unwrap() = Some(root.clone());
            let id = service.repo.new_scan_id().unwrap();
            service.repo.insert_scan(&id, &root, false).unwrap();
            let job = JobRuntime::new(JobEvent::new(&id, &id, "scan", 1), None);
            let started = Instant::now();
            let error = if overlap {
                writer_pipeline::scan(
                    &service,
                    &id,
                    &root,
                    &job,
                    true,
                    readers,
                    READER_QUEUE_CHUNKS,
                )
            } else {
                scan_sync_with_options(&service, &id, &root, &job, true, readers)
            }
            .unwrap_err();
            assert_eq!(error.code, "WORKER_FAILED");
            assert!(started.elapsed() < Duration::from_secs(1));
            assert_eq!(service.repo.scan(&id).unwrap().discovered_count, "1");
        }
    }
    #[cfg(windows)]
    #[test]
    fn cancelled_deferred_directory_retains_explicit_unknown_metadata_after_recovery() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("source");
        fs::create_dir_all(root.join("child")).unwrap();
        fs::write(root.join("child/inside.bin"), b"own").unwrap();
        let root = platform::absolute(&root).unwrap();
        let child = root.join("child");
        let data = fixture.path().join("data");
        let service = Service::new(&data).unwrap();
        let (entered_tx, entered_rx) = crossbeam_channel::bounded(1);
        let (release_tx, release_rx) = crossbeam_channel::bounded(1);
        let (stopped_tx, stopped_rx) = crossbeam_channel::bounded(1);
        *DEFERRED_DIRECTORY_HOOK.lock().unwrap() =
            Some((child.clone(), entered_tx, release_rx, stopped_tx));
        struct Clear;
        impl Drop for Clear {
            fn drop(&mut self) {
                *DEFERRED_DIRECTORY_HOOK.lock().unwrap() = None;
            }
        }
        let _clear = Clear;
        let scan = service.start_scan(root.clone(), None).unwrap();
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let c = service.repo.connect().unwrap();
        // Staged dispatch may enter the reader hook before its row commits.
        // Inspect only the committed prefix, retaining the metadata barrier.
        let raw = crate::stored_path::encode(&child, crate::stored_path::Mode::from_environment())
            .unwrap();
        let start = Instant::now();
        let id: i64 = loop {
            if let Some(id) = c
                .query_row(
                    "SELECT id FROM nodes WHERE scan_id=?1 AND path=?2",
                    params![scan.scan_id, raw],
                    |r| r.get(0),
                )
                .map(Some)
                .or_else(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    error => Err(error),
                })
                .unwrap()
            {
                break id;
            }
            assert!(start.elapsed() < Duration::from_secs(3));
            std::thread::sleep(Duration::from_millis(5));
        };
        let partial = service
            .repo
            .node(&scan.scan_id, &id.to_string(), None)
            .unwrap();
        assert!(partial.node.link_count.is_none());
        assert!(partial.node.identity.is_some());
        assert!(partial.node.incomplete);
        assert_eq!(
            crate::repository::check_directory_metadata_complete(&c, &scan.scan_id)
                .unwrap_err()
                .code,
            "METADATA_INCOMPLETE"
        );
        let start = Instant::now();
        service.cancel_job(&scan.scan_id).unwrap();
        stopped_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        release_tx.send(()).unwrap();
        let event = terminal(&service, &scan.scan_id);
        assert_eq!(event.status, "cancelled");
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(
            service
                .repo
                .node(&scan.scan_id, &id.to_string(), None)
                .unwrap()
                .node
                .link_count,
            None
        );
        drop(c);
        drop(service);
        let recovered = Service::new(&data).unwrap();
        let saved = recovered
            .repo
            .node(&scan.scan_id, &id.to_string(), None)
            .unwrap();
        assert!(saved.node.incomplete);
        assert!(saved.node.link_count.is_none());
        assert_eq!(
            recovered.repo.scan(&scan.scan_id).unwrap().status,
            "cancelled"
        );
        let c = recovered.repo.connect().unwrap();
        assert_eq!(
            crate::repository::check_directory_metadata_complete(&c, &scan.scan_id)
                .unwrap_err()
                .code,
            "METADATA_INCOMPLETE"
        );
    }
    #[test]
    fn deferred_metadata_never_accepts_a_nonrecursive_directory_with_the_same_identity() {
        let fixture = tempfile::tempdir().unwrap();
        let mut measured = platform::metadata(fixture.path()).unwrap();
        let expected = measured.identity.clone();
        assert!(matches_directory_metadata(&measured, &expected));
        measured.recurse = false;
        measured.cloud_state = Some("placeholder_not_recalled".into());
        assert!(!matches_directory_metadata(&measured, &expected));
        measured.cloud_state = None;
        measured.attributes |= 0x1000;
        assert!(!matches_directory_metadata(&measured, &expected));
        measured.recurse = true;
        measured.identity = Some("different".into());
        assert!(!matches_directory_metadata(&measured, &expected));
    }

    #[cfg(windows)]
    #[test]
    fn bulk_and_legacy_scans_keep_directory_metadata_and_exact_tree_counts() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("source");
        fs::create_dir_all(root.join("branch/child")).unwrap();
        fs::write(root.join("branch/child/中文🙂.bin"), b"abcdef").unwrap();
        fs::hard_link(
            root.join("branch/child/中文🙂.bin"),
            root.join("hardlink.bin"),
        )
        .unwrap();
        fs::write(root.join("empty.bin"), b"").unwrap();
        let root = platform::absolute(&root).unwrap();
        let service = Service::new(&fixture.path().join("data")).unwrap();
        let run = |fast, readers, slots, overlap| {
            let id = service.repo.new_scan_id().unwrap();
            service.repo.insert_scan(&id, &root, false).unwrap();
            let job = JobRuntime::new(JobEvent::new(&id, &id, "scan", 1), None);
            let scan = if overlap {
                writer_pipeline::scan(&service, &id, &root, &job, fast, readers, slots)
            } else {
                scan_sync_serial_with_queue_capacity(
                    &service, &id, &root, &job, fast, readers, slots,
                )
            }
            .unwrap();
            assert!(job.terminal());
            assert_eq!(scan.status, "completed");
            let connection = service.repo.connect().unwrap();
            let mut stmt = connection
                .prepare("SELECT id FROM nodes WHERE scan_id=?1")
                .unwrap();
            let ids = stmt
                .query_map([&id], |r| r.get::<_, i64>(0))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap();
            let nodes = ids
                .into_iter()
                .map(|n| service.repo.node(&id, &n.to_string(), None).unwrap())
                .map(|n| (platform::encode_path(&n.path), n))
                .collect::<std::collections::BTreeMap<_, _>>();
            (scan, nodes)
        };
        let (old, legacy) = run(false, 2, 32, false);
        let (new, bulk) = run(true, 2, 32, false);
        for (readers, slots) in [
            (4, 32),
            (4, 64),
            (4, 128),
            (8, 32),
            (8, 64),
            (8, 128),
            (16, 32),
            (16, 64),
            (16, 128),
            (1, 32),
            (3, 32),
            (12, 32),
            (24, 32),
            (64, 32),
        ] {
            for overlap in [false, true] {
                let (parallel_scan, parallel) = run(true, readers, slots, overlap);
                assert_eq!(new.discovered_count, parallel_scan.discovered_count);
                assert_eq!(new.logical_total_bytes, parallel_scan.logical_total_bytes);
                assert_eq!(new.error_count, parallel_scan.error_count);
                assert!(bulk.keys().eq(parallel.keys()));
                for (path, node) in &bulk {
                    let parent_path =
                        |nodes: &std::collections::BTreeMap<Vec<u8>, StoredNode>,
                         parent: &Option<String>| {
                            parent.as_ref().map(|id| {
                                nodes
                                    .values()
                                    .find(|n| &n.node.node_id == id)
                                    .map(|n| platform::encode_path(&n.path))
                                    .unwrap()
                            })
                        };
                    assert_eq!(
                        parent_path(&bulk, &node.node.parent_id),
                        parent_path(&parallel, &parallel[path].node.parent_id)
                    );
                    let mut baseline = node.node.clone();
                    let mut concurrent = parallel[path].node.clone();
                    // Discovery IDs and revisions are scheduling details. Every saved
                    // path, own metadata and aggregate remains identical.
                    baseline.node_id.clear();
                    concurrent.node_id.clear();
                    baseline.parent_id = None;
                    concurrent.parent_id = None;
                    assert_eq!(
                        serde_json::to_value(baseline).unwrap(),
                        serde_json::to_value(concurrent).unwrap()
                    );
                }
            }
        }
        assert_eq!(old.discovered_count, new.discovered_count);
        assert_eq!(old.logical_total_bytes, new.logical_total_bytes);
        assert_eq!(old.error_count, new.error_count);
        assert_eq!(bulk.len(), 6);
        assert!(legacy.keys().eq(bulk.keys()));
        for (path, stored) in &bulk {
            let old = &legacy[path].node;
            let node = &stored.node;
            assert_eq!(node.kind, old.kind);
            assert_eq!(node.depth, old.depth);
            assert_eq!(node.logical_bytes, old.logical_bytes);
            assert_eq!(node.logical_total_bytes, old.logical_total_bytes);
            assert_eq!(node.file_count, old.file_count);
            assert_eq!(node.directory_count, old.directory_count);
            assert_eq!(node.children_count, old.children_count);
            assert_eq!(node.error_code, old.error_code);
            assert_eq!(node.incomplete, old.incomplete);
            assert_eq!(node.link_count, old.link_count);
            assert_eq!(node.modified_at, old.modified_at);
            let measured = platform::metadata(&stored.path).unwrap();
            assert_eq!(node.identity, measured.identity);
            if node.kind == "file" {
                assert_eq!(
                    node.allocated_bytes,
                    measured.allocated.map(|b| b.to_string())
                );
            }
            if node.kind == "directory" {
                assert_eq!(node.identity, old.identity);
            }
        }
        let root_node = &bulk[&platform::encode_path(&root)].node;
        let sum = bulk
            .values()
            .filter(|n| n.node.kind == "file")
            .map(|n| parse_bytes(n.node.allocated_bytes.as_ref().unwrap()).unwrap())
            .sum::<i64>();
        assert_eq!(root_node.allocated_known_total_bytes, sum.to_string());
        assert_eq!(root_node.allocated_unknown_count, "0");
        assert_eq!(root_node.logical_total_bytes, "12");
    }

    #[cfg(windows)]
    #[test]
    fn bulk_and_legacy_acl_failures_keep_exact_coverage_and_refresh_completion() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        let denied = source.join("denied");
        fs::create_dir_all(&denied).unwrap();
        fs::write(denied.join("child"), b"abc").unwrap();
        let principal = std::env::var("USERNAME").unwrap();
        let grant = std::process::Command::new("icacls")
            .arg(&denied)
            .args(["/deny", &format!("{principal}:(OI)(CI)(R)")])
            .output()
            .unwrap();
        assert!(grant.status.success(), "{grant:?}");
        struct Restore(std::path::PathBuf, String);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::process::Command::new("icacls")
                    .arg(&self.0)
                    .args(["/remove:d", &self.1])
                    .output();
            }
        }
        let restore = Restore(denied.clone(), principal);
        let root = platform::absolute(&source).unwrap();
        let child_path = root.join("denied");
        let service = Service::new(&fixture.path().join("data")).unwrap();
        let counters = |n: &Node| {
            [
                n.logical_total_bytes.clone(),
                n.allocated_known_total_bytes.clone(),
                n.allocated_unknown_count.clone(),
                n.file_count.clone(),
                n.directory_count.clone(),
                n.children_count.clone(),
            ]
        };
        let mut scans = vec![];
        for fast in [false, true] {
            let id = service.repo.new_scan_id().unwrap();
            service.repo.insert_scan(&id, &root, false).unwrap();
            let job = JobRuntime::new(JobEvent::new(&id, &id, "scan", 1), None);
            let scan = scan_sync_with_mode(&service, &id, &root, &job, fast).unwrap();
            assert!(job.terminal());
            assert_eq!(scan.status, "completed_with_errors");
            assert_eq!(scan.error_count, "1");
            assert_eq!(scan.discovered_count, "2");
            assert_eq!(scan.logical_total_bytes, "0");
            let c = service.repo.connect().unwrap();
            let child_id: i64 = c
                .query_row(
                    "SELECT id FROM nodes WHERE scan_id=?1 AND path=?2",
                    params![
                        id,
                        crate::stored_path::encode(
                            &child_path,
                            crate::stored_path::Mode::from_environment()
                        )
                        .unwrap()
                    ],
                    |r| r.get(0),
                )
                .unwrap();
            crate::repository::check_directory_metadata_complete(&c, &id).unwrap();
            let child = service
                .repo
                .node(&id, &child_id.to_string(), None)
                .unwrap()
                .node;
            let root_node = service.repo.node(&id, "1", None).unwrap().node;
            assert_eq!(child.error_code.as_deref(), Some("ACCESS_DENIED"));
            assert!(child.incomplete && root_node.incomplete);
            assert_eq!(child.link_count, None);
            if fast {
                assert_eq!(child.kind, "directory");
                assert!(child.identity.is_some());
                assert_eq!(child.allocated_bytes.as_deref(), Some("0"));
                assert_eq!(counters(&child), ["0", "0", "0", "0", "0", "0"]);
                assert_eq!(counters(&root_node), ["0", "0", "0", "0", "1", "1"]);
            } else {
                assert_eq!(child.kind, "other");
                assert_eq!(child.identity, None);
                assert_eq!(child.allocated_bytes, None);
                assert_eq!(counters(&child), ["0", "0", "1", "0", "0", "0"]);
                assert_eq!(counters(&root_node), ["0", "0", "1", "0", "0", "1"]);
            }
            scans.push((scan, child));
        }
        drop(restore);
        let measured = platform::metadata(&child_path).unwrap();
        let allocation = platform::metadata(&child_path.join("child"))
            .unwrap()
            .allocated
            .unwrap()
            .to_string();
        for (scan, old_child) in scans {
            service
                .refresh(scan.scan_id.clone(), old_child.node_id.clone(), None)
                .unwrap();
            let event = terminal(&service, &scan.scan_id);
            assert_eq!(event.status, "completed", "{event:?}");
            let current = service.repo.scan(&scan.scan_id).unwrap();
            assert_eq!(current.status, "completed");
            assert_eq!(current.error_count, "0");
            let child = service
                .repo
                .node(&scan.scan_id, &old_child.node_id, None)
                .unwrap()
                .node;
            let root_node = service.repo.node(&scan.scan_id, "1", None).unwrap().node;
            assert_eq!(child.kind, "directory");
            assert_eq!(child.identity, measured.identity);
            assert_eq!(child.link_count, measured.links.map(|n| n.to_string()));
            assert_eq!(child.error_code, None);
            assert!(!child.incomplete && !root_node.incomplete);
            assert_eq!(counters(&child), ["3", &allocation, "0", "1", "0", "1"]);
            assert_eq!(counters(&root_node), ["3", &allocation, "0", "1", "1", "1"]);
            let c = service.repo.connect().unwrap();
            crate::repository::check_directory_metadata_complete(&c, &scan.scan_id).unwrap();
        }
    }

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
        let _control = READER_CONTROL_TEST_LOCK.lock().unwrap();
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("live.txt"), b"abc").unwrap();
        let root = platform::absolute(&source).unwrap();
        let _mode = writer_pipeline::test_writer_mode(&root, false);
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
    fn oversized_entry_chunks_report_fatal_and_join_without_leaking_permits() {
        // Lower only the fixture's entry budget to exercise both chunk send
        // sites with ordinary files, without claiming Windows emitted 8 MiB.
        for count in [1, READER_CHUNK_ENTRIES] {
            let fixture = tempfile::tempdir().unwrap();
            let source = fixture.path().join("source");
            fs::create_dir(&source).unwrap();
            for i in 0..count {
                fs::write(source.join(format!("file-{i:04}")), b"own fixture").unwrap();
            }
            let root = platform::absolute(&source).unwrap();
            let budget = Arc::new(ReaderBudget {
                limit: 64,
                ..ReaderBudget::default()
            });
            let auxiliary = Arc::new(ReaderBudget {
                limit: AUXILIARY_MESSAGE_BYTES,
                ..ReaderBudget::default()
            });
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let stop = std::sync::atomic::AtomicBool::new(false);
            let (requests_tx, requests_rx) = crossbeam_channel::bounded(32);
            let (output_tx, output_rx) = crossbeam_channel::bounded(32);
            requests_tx.send((1, root.clone(), 0, None, false)).unwrap();
            std::thread::scope(|scope| {
                let mut readers = Vec::new();
                for _ in 0..2 {
                    let requests = requests_rx.clone();
                    let budget = budget.clone();
                    let output = ReaderOutput {
                        sender: output_tx.clone(),
                        observation: None,
                        auxiliary_budget: auxiliary.clone(),
                        cancel: &cancel,
                        path_codec: crate::stored_path::Mode::Utf16,
                        metadata_codec: crate::stored_metadata::Mode::Json,
                        file_path_storage: crate::stored_path::FileStorage::Full,
                    };
                    let reader = platform::ScanDirectoryReader::new(&root, true);
                    readers.push(scope.spawn(|| {
                        read_directories(requests, output, &cancel, &stop, false, budget, reader)
                    }));
                }
                let message = output_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                let DirectoryMessage::Fatal(error) = message.message else {
                    panic!("oversized entry chunk did not explicitly fail the request");
                };
                assert_eq!(error.code, "READER_PAYLOAD_BUDGET");
                drop(requests_tx);
                drop(output_rx);
                budget.stop();
                auxiliary.stop();
                for reader in readers {
                    reader.join().unwrap();
                }
            });
            assert_eq!(budget.state.lock().unwrap().0, 0);
            assert_eq!(auxiliary.state.lock().unwrap().0, 0);
            assert_eq!(fs::read_dir(&source).unwrap().count(), count);
        }
    }

    #[test]
    fn profiled_channel_wait_preserves_order_and_clears_after_disconnect() {
        let probe = Arc::new(ReaderWaitProbe::default());
        let (sender, receiver) = crossbeam_channel::bounded(1);
        sender
            .send(ReaderMessage {
                message: DirectoryMessage::Done(
                    0,
                    ReaderTimings {
                        enumerate_ms: 0.0,
                        ..Default::default()
                    },
                ),
                auxiliary_permit: None,
            })
            .unwrap();
        let filler = sender.clone();
        let (release_tx, release_rx) = crossbeam_channel::bounded(1);
        let producer_probe = probe.clone();
        let producer = std::thread::spawn(move || {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let output = ReaderOutput {
                sender,
                observation: Some(producer_probe),
                auxiliary_budget: Arc::new(ReaderBudget {
                    limit: AUXILIARY_MESSAGE_BYTES,
                    ..Default::default()
                }),
                cancel: &cancel,
                path_codec: crate::stored_path::Mode::Utf16,
                metadata_codec: crate::stored_metadata::Mode::Json,
                file_path_storage: crate::stored_path::FileStorage::Full,
            };
            for value in [1.0, 2.0] {
                if output
                    .send(DirectoryMessage::Done(
                        0,
                        ReaderTimings {
                            enumerate_ms: value,
                            ..Default::default()
                        },
                    ))
                    .is_err()
                {
                    return value;
                }
                if value == 1.0 {
                    release_rx.recv().unwrap();
                }
            }
            0.0
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while probe.snapshot().active_waiters[1] == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(probe.snapshot().active_waiters[1], 1);
        for expected in [0.0, 1.0] {
            let DirectoryMessage::Done(_, timing) = receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .message
            else {
                panic!("unexpected message")
            };
            assert_eq!(timing.enumerate_ms, expected);
        }
        filler
            .send(ReaderMessage {
                message: DirectoryMessage::Done(
                    0,
                    ReaderTimings {
                        enumerate_ms: 99.0,
                        ..Default::default()
                    },
                ),
                auxiliary_permit: None,
            })
            .unwrap();
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while probe.snapshot().message_send_waits < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(receiver);
        assert_eq!(producer.join().unwrap(), 2.0);
        let saved = probe.snapshot();
        assert_eq!(saved.active_waiters, [0, 0]);
        assert_eq!(saved.message_send_waits, 2);
        assert_eq!(saved.message_send_waits_by_kind, [0, 0, 2, 0, 0]);
    }

    #[test]
    fn auxiliary_message_budget_charges_owned_capacity_and_releases_both_permits() {
        for slots in [32, 64, 128] {
            let (sender, receiver) = crossbeam_channel::bounded(slots);
            let auxiliary = Arc::new(ReaderBudget {
                limit: AUXILIARY_MESSAGE_BYTES,
                ..Default::default()
            });
            let entries = Arc::new(ReaderBudget::default());
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let output = ReaderOutput {
                sender,
                observation: None,
                auxiliary_budget: auxiliary.clone(),
                cancel: &cancel,
                path_codec: crate::stored_path::Mode::Utf16,
                metadata_codec: crate::stored_metadata::Mode::Json,
                file_path_storage: crate::stored_path::FileStorage::Full,
            };
            let mut text = String::with_capacity(512);
            text.push_str("saved error");
            let mut path = PathBuf::with_capacity(256);
            path.push("source");
            let error = DirectoryMessage::Error {
                parent: 1,
                path,
                stage: "enumerate",
                error: AppError::new("IO_ERROR", text),
            };
            let expected_error = error.auxiliary_bytes();
            output.send(error).unwrap();
            let mut parent_path = PathBuf::with_capacity(128);
            parent_path.push("parent");
            let expected_parent = parent_path.capacity();
            output
                .send(DirectoryMessage::Entries {
                    parent: 1,
                    path: parent_path,
                    depth: 0,
                    entries: vec![],
                    permit: entries.acquire(32, &cancel).unwrap(),
                })
                .unwrap();
            let auxiliary_used = auxiliary.state.lock().unwrap().0;
            let entries_used = entries.state.lock().unwrap().0;
            assert_eq!(auxiliary_used, expected_error + expected_parent);
            assert_eq!(entries_used, 32);
            let received = receiver.recv().unwrap();
            assert_eq!(received.message.auxiliary_bytes(), expected_error);
            drop(received);
            let auxiliary_used = auxiliary.state.lock().unwrap().0;
            assert_eq!(auxiliary_used, expected_parent);
            drop(receiver);
            // A disconnected queue retains owned messages while a sender remains.
            let auxiliary_used = auxiliary.state.lock().unwrap().0;
            let entries_used = entries.state.lock().unwrap().0;
            assert_eq!(auxiliary_used, expected_parent);
            assert_eq!(entries_used, 32);
            drop(output);
            let auxiliary_used = auxiliary.state.lock().unwrap().0;
            let entries_used = entries.state.lock().unwrap().0;
            assert_eq!(auxiliary_used, 0);
            assert_eq!(entries_used, 0);
        }
    }

    #[test]
    fn binary_metadata_encoding_cap_uses_the_real_reader_encoder_and_emits_fatal() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("hint.bin");
        fs::write(&file, b"abc").unwrap();
        let mut metadata = platform::metadata(&file).unwrap();
        metadata.link_target = Some("x".repeat(crate::stored_metadata::MAX_RECORD_BYTES));
        let entry = platform::DirectoryEntry {
            path: file,
            hint: Some(metadata),
        };
        assert!(entry.scan_file_metadata().is_some());
        let auxiliary = Arc::new(ReaderBudget {
            limit: AUXILIARY_MESSAGE_BYTES,
            ..Default::default()
        });
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let (sender, receiver) = crossbeam_channel::bounded(2);
        let output = ReaderOutput {
            sender,
            observation: None,
            auxiliary_budget: auxiliary.clone(),
            cancel: &cancel,
            path_codec: crate::stored_path::Mode::Wtf8,
            metadata_codec: crate::stored_metadata::Mode::Binary,
            file_path_storage: crate::stored_path::FileStorage::Full,
        };
        assert!(matches!(
            output.encode_entry(1, entry.path.parent().unwrap(), &entry),
            Err(EntryEncodingError::Stop)
        ));
        let message = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        let DirectoryMessage::Fatal(error) = &message.message else {
            panic!("encoding budget must fail the job, not skip a row")
        };
        assert_eq!(error.code, "STORED_METADATA_ENCODING");
        assert!(receiver.try_recv().is_err());
        drop(message);
        drop(output);
        drop(receiver);
        assert_eq!(auxiliary.state.lock().unwrap().0, 0);
    }
    #[test]
    fn basename_reader_uses_trusted_native_prefix_and_actual_capacity_budget() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("metadata.bin");
        std::fs::write(&file, b"abc").unwrap();
        let mut m = platform::metadata(&file).unwrap();
        m.allocation_method = Some("FILE_ID_EXTD_DIR_INFO.AllocationSize".into());
        let parent = PathBuf::from(r"C:\Parent-Case");
        let path = parent.join("中".repeat(2048));
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let auxiliary = Arc::new(ReaderBudget {
            limit: AUXILIARY_MESSAGE_BYTES,
            ..Default::default()
        });
        let (sender, receiver) = crossbeam_channel::bounded(2);
        let output = ReaderOutput {
            sender,
            observation: None,
            auxiliary_budget: auxiliary.clone(),
            cancel: &cancel,
            path_codec: crate::stored_path::Mode::Wtf8,
            metadata_codec: crate::stored_metadata::Mode::Binary,
            file_path_storage: crate::stored_path::FileStorage::Basename,
        };
        let entry = platform::DirectoryEntry {
            path: path.clone(),
            hint: Some(m.clone()),
        };
        let encoded = output.encode_entry(1, &parent, &entry).unwrap().unwrap();
        assert!(crate::stored_path::is_component(
            encoded.path_blob_for_test()
        ));
        assert_eq!(
            crate::stored_path::decode_node(
                encoded.path_blob_for_test().to_vec(),
                "file",
                Some(1),
                Some(crate::stored_path::encode(&parent, crate::stored_path::Mode::Wtf8).unwrap())
            )
            .unwrap(),
            path
        );
        let fallback = output
            .encode_entry(1, Path::new(r"C:\Other"), &entry)
            .unwrap()
            .unwrap();
        assert!(!crate::stored_path::is_component(
            fallback.path_blob_for_test()
        ));
        let mut untrusted = entry;
        untrusted.hint.as_mut().unwrap().allocation_method =
            Some("FILE_STANDARD_INFO.AllocationSize".into());
        assert!(!crate::stored_path::is_component(
            output
                .encode_entry(1, &parent, &untrusted)
                .unwrap()
                .unwrap()
                .path_blob_for_test()
        ));
        let mut entries = Vec::with_capacity(1);
        let bytes = path.capacity()
            + metadata_owned_bytes(Some(&m))
            + encoded.bytes()
            + entries.capacity()
                * std::mem::size_of::<(platform::DirectoryEntry, Option<EncodedNode>)>();
        entries.push((
            platform::DirectoryEntry {
                path,
                hint: Some(m),
            },
            Some(encoded),
        ));
        let budget = Arc::new(ReaderBudget {
            limit: bytes - 1,
            ..Default::default()
        });
        assert!(output
            .send_entries(1, parent, 0, entries, bytes, &budget)
            .is_err());
        let message = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        let DirectoryMessage::Fatal(error) = &message.message else {
            panic!("actual component capacity must reject over-budget chunk");
        };
        assert_eq!(error.code, "READER_PAYLOAD_BUDGET");
        drop(message);
        drop(output);
        drop(receiver);
        assert_eq!(budget.state.lock().unwrap().0, 0);
        assert_eq!(auxiliary.state.lock().unwrap().0, 0);
    }
    #[test]
    fn wtf8_unicode_entry_uses_actual_capacity_and_is_rejected_before_exceeding_reader_budget() {
        for metadata_codec in [
            crate::stored_metadata::Mode::Json,
            crate::stored_metadata::Mode::Binary,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let file = temp.path().join("metadata.bin");
            std::fs::write(&file, b"abc").unwrap();
            let mut metadata = platform::metadata(&file).unwrap();
            metadata.link_target = Some("unknown-target中文".repeat(1024));
            let path = PathBuf::from(format!("C:\\{}", "中".repeat(2048)));
            let encoded = encode_node_with_codecs(
                &path,
                &metadata,
                crate::stored_path::Mode::Wtf8,
                metadata_codec,
            )
            .unwrap();
            let mut entries = Vec::with_capacity(1);
            let bytes = path.capacity()
                + metadata_owned_bytes(Some(&metadata))
                + encoded.bytes()
                + entries.capacity()
                    * std::mem::size_of::<(platform::DirectoryEntry, Option<EncodedNode>)>();
            entries.push((
                platform::DirectoryEntry {
                    path,
                    hint: Some(metadata),
                },
                Some(encoded),
            ));
            let budget = Arc::new(ReaderBudget {
                limit: bytes - 1,
                ..Default::default()
            });
            let auxiliary = Arc::new(ReaderBudget {
                limit: AUXILIARY_MESSAGE_BYTES,
                ..Default::default()
            });
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let (sender, receiver) = crossbeam_channel::bounded(2);
            let output = ReaderOutput {
                sender,
                observation: None,
                auxiliary_budget: auxiliary.clone(),
                cancel: &cancel,
                path_codec: crate::stored_path::Mode::Wtf8,
                metadata_codec,
                file_path_storage: crate::stored_path::FileStorage::Full,
            };
            assert!(output
                .send_entries(1, PathBuf::from("C:\\"), 0, entries, bytes, &budget)
                .is_err());
            let message = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
            let DirectoryMessage::Fatal(error) = &message.message else {
                panic!("oversized entry was not rejected")
            };
            assert_eq!(error.code, "READER_PAYLOAD_BUDGET");
            drop(message);
            drop(output);
            drop(receiver);
            assert_eq!(budget.state.lock().unwrap().0, 0);
            assert_eq!(auxiliary.state.lock().unwrap().0, 0);
        }
    }

    #[test]
    fn oversized_auxiliary_message_is_fatal_and_cancelled_budget_wait_does_not_enqueue() {
        let (sender, receiver) = crossbeam_channel::bounded(32);
        let auxiliary = Arc::new(ReaderBudget {
            limit: 64,
            ..Default::default()
        });
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let output = ReaderOutput {
            sender: sender.clone(),
            observation: None,
            auxiliary_budget: auxiliary.clone(),
            cancel: &cancel,
            path_codec: crate::stored_path::Mode::Utf16,
            metadata_codec: crate::stored_metadata::Mode::Json,
            file_path_storage: crate::stored_path::FileStorage::Full,
        };
        let oversized = DirectoryMessage::Fatal(AppError::new("ERROR", String::with_capacity(128)));
        assert!(output.send(oversized).is_err());
        let DirectoryMessage::Fatal(error) = receiver.recv().unwrap().message else {
            panic!("oversized message did not fail the job")
        };
        assert_eq!(error.code, "READER_PAYLOAD_BUDGET");
        assert_eq!(auxiliary.state.lock().unwrap().0, 0);
        let permit = auxiliary.acquire(64, &cancel).unwrap();
        let waiting_budget = auxiliary.clone();
        let waiting_cancel = cancel.clone();
        let worker = std::thread::spawn(move || {
            let output = ReaderOutput {
                sender,
                observation: None,
                auxiliary_budget: waiting_budget,
                cancel: &waiting_cancel,
                path_codec: crate::stored_path::Mode::Utf16,
                metadata_codec: crate::stored_metadata::Mode::Json,
                file_path_storage: crate::stored_path::FileStorage::Full,
            };
            output
                .send(DirectoryMessage::Fatal(AppError::new("E", "x")))
                .is_err()
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while auxiliary.state.lock().unwrap().3 == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(auxiliary.state.lock().unwrap().3, 1);
        cancel.store(true, Ordering::Release);
        auxiliary.stop();
        assert!(worker.join().unwrap());
        drop(permit);
        assert_eq!(auxiliary.state.lock().unwrap().0, 0);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn profiled_byte_budget_measures_commit_intersection_and_stop_releases_waiters() {
        let probe = Arc::new(ReaderWaitProbe::default());
        let budget = Arc::new(ReaderBudget {
            observation: Some(probe.clone()),
            ..Default::default()
        });
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let permit = budget.acquire(READER_DATA_BYTES, &cancel).unwrap();
        let mut readers = Vec::new();
        for _ in 0..2 {
            let budget = budget.clone();
            let cancel = cancel.clone();
            readers.push(std::thread::spawn(move || {
                budget.acquire(1, &cancel).is_none()
            }));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while probe.snapshot().active_waiters[0] < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(probe.snapshot().active_waiters[0], 2);
        let flush = probe.begin(ReaderWaitRole::Flush);
        let commit = probe.begin(ReaderWaitRole::Commit);
        std::thread::sleep(Duration::from_millis(5));
        drop(commit);
        drop(flush);
        budget.stop();
        for reader in readers {
            assert!(reader.join().unwrap());
        }
        drop(permit);
        let saved = probe.snapshot();
        assert_eq!(saved.active_waiters, [0, 0]);
        assert_eq!(saved.byte_permit_waits, 2);
        assert_eq!(saved.commit_windows, 1);
        assert_eq!(saved.flush_windows, 1);
        assert!(saved.commit_window_ms > 0.0);
        assert!(saved.readers_blocked_during_flush_ms >= saved.readers_blocked_during_commit_ms);
        assert!(
            saved.byte_permit_waiter_ms_during_commit
                >= saved.byte_permit_blocked_during_commit_ms * 1.99
        );
        assert_eq!(
            saved.readers_blocked_during_commit_ms,
            saved.byte_permit_blocked_during_commit_ms
        );
        assert_eq!(saved.message_send_blocked_during_commit_ms, 0.0);
        assert_eq!(budget.state.lock().unwrap().0, 0);
    }

    #[test]
    fn cancellation_before_dispatch_never_reports_closed_reader_as_failure() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        fs::create_dir(&source).unwrap();
        let root = platform::absolute(&source).unwrap();
        let _mode = writer_pipeline::test_writer_mode(&root, false);
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
        fs::create_dir(&source).unwrap();
        #[cfg(windows)]
        {
            let target = fixture.path().join("target");
            fs::create_dir(&target).unwrap();
            assert!(std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(source.join("panic-directory"))
                .arg(target)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success());
        }
        #[cfg(not(windows))]
        fs::create_dir(source.join("panic-directory")).unwrap();
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
        let _control = READER_CONTROL_TEST_LOCK.lock().unwrap();
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

#[cfg(test)]
mod directory_queue_tests {
    use super::*;

    fn connection(count: i64) -> rusqlite::Connection {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE pending_dirs(scan_id TEXT NOT NULL,id INTEGER NOT NULL,path BLOB NOT NULL,depth INTEGER NOT NULL,expected_identity TEXT,metadata_deferred INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(scan_id,id)) WITHOUT ROWID; CREATE TEMP TABLE consumed_dirs(id INTEGER PRIMARY KEY);").unwrap();
        for id in 1..=count {
            c.execute(
                "INSERT INTO pending_dirs(scan_id,id,path,depth) VALUES('s',?1,?2,?1)",
                params![id, id.to_le_bytes().as_slice()],
            )
            .unwrap();
        }
        c.execute(
            "INSERT INTO pending_dirs(scan_id,id,path,depth) VALUES('other',?1,?2,0)",
            params![count + 2, b"other".as_slice()],
        )
        .unwrap();
        c
    }

    #[test]
    fn one_directory_refills_match_legacy_order_across_flush_and_new_ids() {
        for flush_after in [13, 256, 1024] {
            let c = connection(1200);
            let mut cursor = DirectoryQueueCursor::default();
            let mut seen = Vec::new();
            let mut queries = 0;
            loop {
                if cursor.needs_refill() {
                    cursor.refill(&c, "s").unwrap();
                    queries += 1;
                }
                assert!(cursor.cache.len() <= DIRECTORY_PREFETCH);
                assert!(cursor.owned_bytes() <= DIRECTORY_CACHE_BYTES);
                let found = cursor.cache.front().cloned();
                let mut stmt = c.prepare_cached(FIRST_PENDING_DIRECTORIES).unwrap();
                let legacy = stmt
                    .query_map(params!["s", 1], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, Vec<u8>>(1)?,
                            row.get::<_, u32>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, bool>(4)?,
                        ))
                    })
                    .unwrap()
                    .next()
                    .transpose()
                    .unwrap();
                assert_eq!(found, legacy);
                let Some((id, path, depth, _, _)) = found else {
                    break;
                };
                assert_eq!(depth, id as u32);
                assert_eq!(path, id.to_le_bytes());
                let mut mark = c
                    .prepare_cached("INSERT INTO consumed_dirs(id) VALUES(?1)")
                    .unwrap();
                assert_eq!(cursor.consume(&mut mark).unwrap().0, id);
                drop(mark);
                seen.push(id);
                if seen.len() == flush_after {
                    let tx = c.unchecked_transaction().unwrap();
                    tx.execute(
                        "DELETE FROM pending_dirs WHERE scan_id='s' AND id IN consumed_dirs",
                        [],
                    )
                    .unwrap();
                    tx.execute("DELETE FROM consumed_dirs", []).unwrap();
                    for id in [2000i64, 2001] {
                        tx.execute(
                            "INSERT INTO pending_dirs(scan_id,id,path,depth) VALUES('s',?1,?2,?1)",
                            params![id, id.to_le_bytes().as_slice()],
                        )
                        .unwrap();
                    }
                    tx.commit().unwrap();
                    cursor.reset_after_flush();
                }
            }
            let expected = ((1201 - flush_after as i64)..=1200)
                .rev()
                .chain([2001, 2000])
                .chain((1..=(1200 - flush_after as i64)).rev())
                .collect::<Vec<_>>();
            assert_eq!(seen, expected);
            assert_eq!(
                seen.iter().collect::<std::collections::BTreeSet<_>>().len(),
                1202
            );
            assert!(
                queries < 45,
                "{queries} queries for 1202 one-at-a-time dispatches"
            );
        }
    }

    #[test]
    fn unread_or_failed_dispatch_does_not_advance_past_queued_directories() {
        let c = connection(40);
        let mut cursor = DirectoryQueueCursor::default();
        cursor.refill(&c, "s").unwrap();
        let first = cursor.cache.clone();
        assert_eq!(cursor.before_id, None);
        assert!(!cursor.needs_refill());
        let mut mark = c
            .prepare_cached("INSERT INTO consumed_dirs(id) VALUES(?1)")
            .unwrap();
        for row in first.iter().take(5) {
            assert_eq!(cursor.consume(&mut mark).unwrap().0, row.0);
        }
        let remainder = cursor.cache.clone();
        assert_eq!(remainder.front().unwrap().0, 35);
        c.execute_batch("CREATE TEMP TRIGGER reject_consumed BEFORE INSERT ON consumed_dirs WHEN NEW.id=35 BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
        assert!(cursor.consume(&mut mark).is_err());
        assert_eq!(cursor.cache, remainder);
        assert_eq!(cursor.before_id, Some(36));
        c.execute_batch("DROP TRIGGER reject_consumed;").unwrap();
        let mut seen = first.iter().take(5).map(|row| row.0).collect::<Vec<_>>();
        loop {
            if cursor.needs_refill() {
                cursor.refill(&c, "s").unwrap();
            }
            if cursor.cache.is_empty() {
                break;
            }
            seen.push(cursor.consume(&mut mark).unwrap().0);
        }
        assert_eq!(seen, (1..=40).rev().collect::<Vec<_>>());
        assert!(!cursor.needs_refill());
    }

    #[test]
    fn cache_bounds_owned_paths_and_dispatches_an_oversized_singleton() {
        let c = connection(0);
        for (id, bytes) in [(1, 110_000), (2, 160_000), (3, 300_000)] {
            c.execute(
                "INSERT INTO pending_dirs VALUES('s',?1,?2,1,'expected-identity',1)",
                params![id, vec![7u8; bytes]],
            )
            .unwrap();
        }
        let mut cursor = DirectoryQueueCursor::default();
        let mut mark = c
            .prepare_cached("INSERT INTO consumed_dirs VALUES(?1)")
            .unwrap();
        for id in [3, 2, 1] {
            cursor.refill(&c, "s").unwrap();
            assert_eq!(cursor.cache.len(), 1);
            assert!(cursor.owned_bytes() <= BATCH_PATH_BYTES);
            if id == 3 {
                assert!(cursor.owned_bytes() > DIRECTORY_CACHE_BYTES);
            } else {
                assert!(cursor.owned_bytes() <= DIRECTORY_CACHE_BYTES);
            }
            let item = cursor.consume(&mut mark).unwrap();
            assert_eq!(item.0, id);
            assert_eq!(item.3.as_deref(), Some("expected-identity"));
            assert!(item.4);
        }
        assert!(!cursor.needs_refill());
        assert_eq!(cursor.payload_bytes, 0);
    }

    #[test]
    fn path_exceeding_singleton_limit_is_rejected_without_consuming() {
        let c = connection(0);
        c.execute(
            "INSERT INTO pending_dirs(scan_id,id,path,depth) VALUES('s',1,?1,0)",
            [vec![7u8; BATCH_PATH_BYTES + 1]],
        )
        .unwrap();
        let mut cursor = DirectoryQueueCursor::default();
        assert_eq!(cursor.refill(&c, "s").unwrap_err().code, "PATH_LIMIT");
        assert!(cursor.cache.is_empty());
        assert_eq!(cursor.before_id, None);
        assert_eq!(
            c.query_row::<i64, _, _>("SELECT COUNT(*) FROM consumed_dirs", [], |r| r.get(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn consumed_override_only_accepts_exact_bounded_comparison_values() {
        assert_eq!(consumed_limit_override(None), 1024);
        assert_eq!(consumed_limit_override(Some("256")), 256);
        assert_eq!(consumed_limit_override(Some("1024")), 1024);
        assert_eq!(consumed_limit_override(Some("2048")), 2048);
        assert_eq!(consumed_limit_override(Some("4096")), 4096);
        for value in [
            None,
            Some("0"),
            Some("4095"),
            Some("8192"),
            Some("4096 "),
            Some("04096"),
            Some("-1"),
            Some("garbage"),
            Some("1024 "),
        ] {
            assert_eq!(consumed_limit_override(value), DEFAULT_CONSUMED_LIMIT);
        }
    }
    #[test]
    fn reader_defaults_use_detected_parallelism_and_checked_dynamic_limits() {
        for detected in [1, 3, 12, 24, 64] {
            for (value, expected) in [("2", 2), ("4", 4), ("8", 8), ("16", 16)] {
                assert_eq!(reader_count_for(Some(value), detected), expected);
            }
            for value in [
                None,
                Some("0"),
                Some("1"),
                Some("3"),
                Some("32"),
                Some("4 "),
                Some("8 "),
                Some("16 "),
                Some("invalid"),
            ] {
                assert_eq!(reader_count_for(value, detected), detected);
            }
            let plan = ReaderPlan::new(detected, DEFAULT_CONSUMED_LIMIT).unwrap();
            assert_eq!(plan.readers, detected);
            assert_eq!(plan.in_flight, detected.max(32));
            assert_eq!(plan.ledger_limit, 2048 + detected.max(32));
        }
        assert_eq!(reader_count_for(None, 0), 1);
        for (readers, consumed) in [(0, 1024), (usize::MAX, 1024), (64, usize::MAX)] {
            assert_eq!(
                ReaderPlan::new(readers, consumed).err().unwrap().code,
                "RESOURCE_LIMIT"
            );
        }
        assert!(overlapping_writer(None));
        assert!(overlapping_writer(Some("unknown")));
        assert!(!overlapping_writer(Some("serial")));
        eprintln!(
            "reader owned layout: Metadata={}, DirectoryEntry={}, EncodedNode={}, chunk slot={}",
            std::mem::size_of::<platform::Metadata>(),
            std::mem::size_of::<platform::DirectoryEntry>(),
            std::mem::size_of::<EncodedNode>(),
            std::mem::size_of::<(platform::DirectoryEntry, Option<EncodedNode>)>()
        );
    }

    #[test]
    fn system_processor_provider_uses_all_groups_and_falls_back_only_on_failure() {
        assert_eq!(
            processor_count_from_providers(128, || panic!("unexpected fallback")),
            128
        );
        assert_eq!(
            processor_count_from_providers(1, || panic!("unexpected fallback")),
            1
        );
        assert_eq!(processor_count_from_providers(0, || 24), 24);
        assert_eq!(processor_count_from_providers(0, || 0), 1);
        let plan = ReaderPlan::new(processor_count_from_providers(128, || 16), 1024).unwrap();
        assert_eq!(plan.in_flight, 128);
        assert_eq!(plan.ledger_limit, 2176);
    }

    #[test]
    fn descending_watermark_uses_the_scan_and_id_primary_key_range() {
        let c = connection(40);
        let mut statement = c
            .prepare(&format!("EXPLAIN QUERY PLAN {NEXT_PENDING_DIRECTORIES}"))
            .unwrap();
        let plan = statement
            .query_map(params!["s", 1, 35], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            plan.iter()
                .any(|detail| detail.contains("PRIMARY KEY")
                    && detail.contains("scan_id=? AND id<?")),
            "{plan:?}"
        );
        assert!(
            plan.iter().all(|detail| !detail.contains("TEMP B-TREE")),
            "{plan:?}"
        );
    }
}
