use super::*;
use crossbeam_channel::{Receiver, Sender};
use std::sync::{atomic::AtomicBool, Arc};

const ERROR_LIMIT: usize = 1024;

#[cfg(test)]
#[derive(Clone)]
struct TestFault {
    scan_id: String,
    data_dir: PathBuf,
    root: Option<PathBuf>,
    before_commit: bool,
    coordinator: bool,
    ack_sequence: Option<u64>,
    entered: Sender<()>,
    release: Receiver<()>,
    output: Option<Sender<ReaderMessage>>,
    budgets: Option<(Arc<ReaderBudget>, Arc<ReaderBudget>, Arc<ReaderBudget>)>,
    close_failure: bool,
    legacy: bool,
    metadata_oversize: bool,
}
#[cfg(test)]
static TEST_FAULT: std::sync::Mutex<Option<TestFault>> = std::sync::Mutex::new(None);
#[cfg(test)]
static TEST_FAULT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(test)]
pub(super) fn test_overlapping_root(root: &Path) -> bool {
    TEST_FAULT
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|fault| fault.root.as_deref() == Some(root))
}
#[cfg(test)]
pub(super) fn test_before_commit(scan_id: &str, data_dir: &Path) {
    let fault = TEST_FAULT
        .lock()
        .unwrap()
        .as_ref()
        .filter(|fault| {
            fault.scan_id == scan_id && fault.data_dir == data_dir && fault.before_commit
        })
        .cloned();
    if let Some(fault) = fault {
        fault.entered.send(()).unwrap();
        fault.release.recv().unwrap();
        panic!("injected writer transaction failure");
    }
}

pub(super) struct PendingScanError {
    id: i64,
    path: PathBuf,
    stage: &'static str,
    error: AppError,
}
impl PendingScanError {
    fn bytes(&self) -> usize {
        self.path.capacity() + error_bytes(&self.error)
    }
}
fn error_bytes(error: &AppError) -> usize {
    error.code.capacity()
        + error.message.capacity()
        + error.node_id.as_ref().map_or(0, String::capacity)
}
fn row_bytes(row: &MetadataResult) -> usize {
    row.2.capacity()
        + row
            .4
            .as_ref()
            .map_or_else(error_bytes, |m| metadata_owned_bytes(Some(m)))
        + row.5.as_ref().map_or(0, EncodedNode::bytes)
}

pub(super) fn persist_errors(
    tx: &rusqlite::Transaction<'_>,
    scan_id: &str,
    errors: &mut Vec<PendingScanError>,
) -> Result<()> {
    for error in errors.drain(..) {
        Repository::error_conn(
            tx,
            scan_id,
            ScanError {
                node_id: Some(error.id.to_string()),
                display_path: platform::display(&error.path),
                stage: error.stage.into(),
                error: error.error,
            },
        )?;
        tx.execute(
            "DELETE FROM directory_metadata_pending WHERE scan_id=?1 AND id=?2",
            params![scan_id, error.id],
        )?;
    }
    Ok(())
}

/// One owned packet is being written and at most one is being collected.
/// Heap capacity is charged separately from these fixed, count-bounded slots.
struct WriteBatch {
    rows: Vec<MetadataResult>,
    metadata: Vec<DirectoryMetadataUpdate>,
    errors: Vec<PendingScanError>,
    consumed: Vec<i64>,
    heap_bytes: usize,
    started: Instant,
}
impl WriteBatch {
    fn new(consumed_limit: usize) -> Self {
        Self {
            rows: Vec::with_capacity(BATCH_ENTRIES),
            metadata: Vec::with_capacity(2048),
            errors: Vec::with_capacity(ERROR_LIMIT),
            consumed: Vec::with_capacity(consumed_limit),
            heap_bytes: 0,
            started: Instant::now(),
        }
    }
    fn empty(&self) -> bool {
        self.rows.is_empty()
            && self.metadata.is_empty()
            && self.errors.is_empty()
            && self.consumed.is_empty()
    }
    fn fits(&self, bytes: usize) -> bool {
        self.heap_bytes.saturating_add(bytes) <= BATCH_PATH_BYTES
    }
    fn full(&self, pending: usize, reserved: usize, consumed_limit: usize) -> bool {
        self.rows.len() + pending >= BATCH_ENTRIES
            || pending >= 2000
            || self.metadata.len() >= 2048
            || self.errors.len() >= ERROR_LIMIT
            || self.consumed.len() >= consumed_limit
            || self.heap_bytes.saturating_add(reserved) >= BATCH_PATH_BYTES
            || !self.empty() && self.started.elapsed() >= Duration::from_millis(100)
    }
}

enum WriteCommand {
    Batch { sequence: u64, batch: WriteBatch },
    Finish,
    Abort,
}
struct CommitAck {
    sequence: u64,
    consumed: Vec<i64>,
}
struct WriterResult {
    scan: Scan,
    profile: ScanProfile,
}

struct WriterPort {
    commands: Sender<WriteCommand>,
    acknowledgements: Receiver<Result<CommitAck>>,
    sequence: u64,
    outstanding: Option<u64>,
    committed_batches: usize,
    unpublished: Vec<i64>,
    ledger_count: usize,
    ledger_limit: usize,
}
impl WriterPort {
    fn apply_ack(&mut self, c: &rusqlite::Connection, ack: CommitAck) -> Result<()> {
        if self.outstanding != Some(ack.sequence) {
            return Err(AppError::new("WRITER_PROTOCOL", "扫描写入确认顺序不一致"));
        }
        let remaining = self
            .ledger_count
            .checked_sub(ack.consumed.len())
            .ok_or_else(|| AppError::new("WRITER_PROTOCOL", "扫描派发记录数量不一致"))?;
        let tx = c.unchecked_transaction()?;
        {
            let mut remove = tx.prepare_cached("DELETE FROM dispatched_dirs WHERE id=?1")?;
            for id in &ack.consumed {
                if remove.execute([id])? != 1 {
                    return Err(AppError::new("WRITER_PROTOCOL", "扫描派发记录确认不一致"));
                }
            }
        }
        tx.execute("DELETE FROM unpublished_dirs", [])?;
        tx.commit()?;
        self.unpublished.clear();
        self.ledger_count = remaining;
        self.outstanding = None;
        self.committed_batches += 1;
        Ok(())
    }
    fn poll(&mut self, c: &rusqlite::Connection) -> Result<bool> {
        if self.outstanding.is_none() {
            return Ok(false);
        }
        match self.acknowledgements.try_recv() {
            Ok(ack) => {
                self.apply_ack(c, ack?)?;
                Ok(true)
            }
            Err(crossbeam_channel::TryRecvError::Empty) => Ok(false),
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                Err(AppError::new("WORKER_FAILED", "扫描写线程已退出"))
            }
        }
    }
    fn wait(&mut self, c: &rusqlite::Connection) -> Result<bool> {
        if self.outstanding.is_none() {
            return Ok(false);
        }
        let ack = self
            .acknowledgements
            .recv()
            .map_err(|_| AppError::new("WORKER_FAILED", "扫描写线程中断"))??;
        self.apply_ack(c, ack)?;
        Ok(true)
    }
    fn submit(
        &mut self,
        c: &rusqlite::Connection,
        batch: &mut WriteBatch,
        limit: usize,
    ) -> Result<()> {
        self.wait(c)?;
        if batch.empty() {
            return Ok(());
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| AppError::new("SIZE_OUT_OF_RANGE", "扫描批次数量超出上限"))?;
        let sequence = self.sequence;
        let packet = std::mem::replace(batch, WriteBatch::new(limit));
        self.unpublished
            .extend(packet.rows.iter().filter_map(|row| {
                row.4
                    .as_ref()
                    .ok()
                    .filter(|metadata| metadata.recurse)
                    .map(|_| row.0)
            }));
        debug_assert!(self.unpublished.len() <= BATCH_ENTRIES);
        {
            let tx = c.unchecked_transaction()?;
            let mut isolate = tx.prepare_cached("INSERT INTO unpublished_dirs(id) VALUES(?1)")?;
            for id in &self.unpublished {
                isolate.execute([id])?;
            }
            drop(isolate);
            tx.commit()?;
        }
        self.commands
            .send(WriteCommand::Batch {
                sequence,
                batch: packet,
            })
            .map_err(|_| AppError::new("WORKER_FAILED", "扫描写线程已退出"))?;
        self.outstanding = Some(sequence);
        Ok(())
    }
}

/// The exact, bounded set of directories in the unacknowledged packet is
/// excluded, including out-of-order low IDs returned by metadata workers.
/// No query is held while waiting on filesystem workers or the writer.
fn refill_visible(
    cursor: &mut DirectoryQueueCursor,
    c: &rusqlite::Connection,
    scan_id: &str,
) -> Result<usize> {
    debug_assert!(cursor.needs_refill());
    cursor.cache.reserve(DIRECTORY_PREFETCH);
    let mut select = c.prepare_cached(
        "SELECT id,path,depth,expected_identity,metadata_deferred FROM pending_dirs
         WHERE scan_id=?1 AND id<?2 AND id NOT IN dispatched_dirs AND id NOT IN unpublished_dirs
         ORDER BY id DESC LIMIT ?3",
    )?;
    let mut rows = select.query(params![
        scan_id,
        cursor.before_id.unwrap_or(i64::MAX),
        DIRECTORY_PREFETCH as i64
    ])?;
    cursor.exhausted = true;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        count += 1;
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
        let bytes = raw.len().saturating_add(expected.map_or(0, str::len));
        if cursor.cache.capacity() * std::mem::size_of::<QueuedDirectory>() + bytes
            > BATCH_PATH_BYTES
        {
            return Err(AppError::new("PATH_LIMIT", "目录队列路径超出读取预算"));
        }
        if !cursor.cache.is_empty() && cursor.owned_bytes() + bytes > DIRECTORY_CACHE_BYTES {
            cursor.exhausted = false;
            break;
        }
        let item = (
            row.get(0)?,
            raw.to_vec(),
            row.get(2)?,
            expected.map(str::to_owned),
            row.get(4)?,
        );
        cursor.payload_bytes += item.1.capacity() + item.3.as_ref().map_or(0, String::capacity);
        cursor.cache.push_back(item);
        if cursor.cache.len() == DIRECTORY_PREFETCH || cursor.owned_bytes() > DIRECTORY_CACHE_BYTES
        {
            cursor.exhausted = false;
            break;
        }
    }
    Ok(count)
}

struct MetadataReply {
    row: MetadataResult,
    _permit: Option<ReaderPermit>,
}

#[allow(clippy::large_enum_variant)] // At most 2000 inline reply slots; avoids a per-result allocation.
enum MetadataMessage {
    Complete(MetadataReply),
    Fatal(AppError),
}
const METADATA_REPLY_BYTES: usize = 4 * 1024 * 1024;
const METADATA_ITEM_BYTES: usize = 1024 * 1024;

struct Collector<'a> {
    service: &'a Service,
    scan_id: &'a str,
    connection: &'a rusqlite::Connection,
    port: &'a mut WriterPort,
    profile: &'a mut ScanProfile,
    batch: WriteBatch,
    queue: DirectoryQueueCursor,
    metadata: &'a Receiver<MetadataMessage>,
    pending: usize,
    reserved_metadata: usize,
    consumed_limit: usize,
}
impl Collector<'_> {
    fn harvest(&mut self) -> Result<()> {
        if self.port.poll(self.connection)? {
            self.queue.reset_after_flush();
        }
        Ok(())
    }
    fn flush_ready(&mut self) -> Result<()> {
        let previous = self.port.committed_batches;
        self.port
            .submit(self.connection, &mut self.batch, self.consumed_limit)?;
        if self.port.committed_batches != previous {
            self.queue.reset_after_flush();
        }
        Ok(())
    }
    fn space(&mut self, bytes: usize) -> Result<()> {
        if bytes > BATCH_PATH_BYTES {
            return Err(AppError::new(
                "READER_PAYLOAD_BUDGET",
                "扫描单项超过内存预算",
            ));
        }
        if !self.batch.fits(bytes) {
            self.flush_ready()?;
        }
        Ok(())
    }
    fn add_row(&mut self, row: MetadataResult) -> Result<()> {
        let bytes = row_bytes(&row);
        self.space(bytes)?;
        if self.batch.rows.len() == BATCH_ENTRIES {
            self.flush_ready()?;
        }
        self.batch.heap_bytes += bytes;
        self.batch.rows.push(row);
        self.profile.peak_batch_path_bytes = self
            .profile
            .peak_batch_path_bytes
            .max(self.batch.heap_bytes);
        Ok(())
    }
    fn receive_metadata(&mut self) -> Result<()> {
        let started = self.profile.enabled.then(Instant::now);
        let message = self
            .metadata
            .recv()
            .map_err(|_| AppError::new("WORKER_FAILED", "元数据工作线程中断"))?;
        if let Some(started) = started {
            self.profile.metadata_wait_ms += milliseconds(started);
        }
        match message {
            MetadataMessage::Fatal(error) => Err(error),
            MetadataMessage::Complete(reply) => {
                self.pending -= 1;
                self.reserved_metadata -= reply.row.2.capacity();
                if reply
                    .row
                    .4
                    .as_ref()
                    .is_err_and(|error| error.code == "WORKER_FAILED")
                {
                    return Err(reply.row.4.unwrap_err());
                }
                // The reply permit remains alive if space() waits for a commit.
                self.add_row(reply.row)?;
                drop(reply._permit);
                Ok(())
            }
        }
    }
    fn flush(&mut self) -> Result<()> {
        while self.pending > 0 {
            self.receive_metadata()?;
        }
        self.flush_ready()
    }
    fn add_metadata(&mut self, update: DirectoryMetadataUpdate) -> Result<()> {
        self.space(update.bytes())?;
        if self.batch.metadata.len() == 2048 {
            self.flush_ready()?;
        }
        self.batch.heap_bytes += update.bytes();
        self.batch.metadata.push(update);
        self.profile.peak_batch_path_bytes = self
            .profile
            .peak_batch_path_bytes
            .max(self.batch.heap_bytes);
        Ok(())
    }
    fn add_error(&mut self, error: PendingScanError) -> Result<()> {
        self.space(error.bytes())?;
        if self.batch.errors.len() == ERROR_LIMIT {
            self.flush_ready()?;
        }
        self.batch.heap_bytes += error.bytes();
        self.batch.errors.push(error);
        self.profile.peak_batch_path_bytes = self
            .profile
            .peak_batch_path_bytes
            .max(self.batch.heap_bytes);
        Ok(())
    }
    fn flush_if_full(&mut self) -> Result<()> {
        if self
            .batch
            .full(self.pending, self.reserved_metadata, self.consumed_limit)
        {
            if self.profile.enabled {
                self.profile.flush_consumed_limit +=
                    usize::from(self.batch.consumed.len() >= self.consumed_limit);
                self.profile.flush_time_limit +=
                    usize::from(self.batch.started.elapsed() >= Duration::from_millis(100));
                self.profile.flush_path_limit +=
                    usize::from(self.batch.heap_bytes + self.reserved_metadata >= BATCH_PATH_BYTES);
                self.profile.flush_entries_limit +=
                    usize::from(self.batch.rows.len() + self.pending >= BATCH_ENTRIES);
                self.profile.flush_metadata_limit +=
                    usize::from(self.pending >= 2000 || self.batch.metadata.len() >= 2048);
            }
            self.flush()?;
        }
        Ok(())
    }
}

fn spawn_metadata_workers(
    work_rx: Receiver<(i64, i64, PathBuf, u32)>,
    replies: Sender<MetadataMessage>,
    budget: Arc<ReaderBudget>,
) -> Vec<std::thread::JoinHandle<()>> {
    let mut workers = Vec::with_capacity(4);
    for _ in 0..4 {
        let receiver = work_rx.clone();
        let output = replies.clone();
        let budget = budget.clone();
        workers.push(std::thread::spawn(move || {
            let accepting = AtomicBool::new(false);
            while let Ok((id, parent, path, depth)) = receiver.recv() {
                #[allow(unused_mut)]
                // Test injection adjusts only an owned result before its budget check.
                let mut measured = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
                #[cfg(test)]
                if TEST_FAULT.lock().unwrap().as_ref().is_some_and(|fault| {
                    fault.metadata_oversize
                        && fault
                            .root
                            .as_ref()
                            .is_some_and(|root| path.starts_with(root))
                }) {
                    if let Ok(metadata) = &mut measured {
                        metadata.kind.reserve_exact(METADATA_ITEM_BYTES + 1);
                    }
                }
                let row = (id, parent, path, depth, measured, None);
                let bytes = row_bytes(&row);
                if bytes > METADATA_ITEM_BYTES {
                    let _ = output.send(MetadataMessage::Fatal(AppError::new(
                        "READER_PAYLOAD_BUDGET",
                        "元数据单项超过内存预算",
                    )));
                    break;
                }
                let Some(permit) = budget.acquire(bytes, &accepting) else {
                    break;
                };
                if output
                    .send(MetadataMessage::Complete(MetadataReply {
                        row,
                        _permit: Some(permit),
                    }))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
    workers
}

fn collect_directories(
    collector: &mut Collector<'_>,
    _root: &Path,
    job: &JobRuntime,
    stop: &AtomicBool,
    requests: &Sender<(i64, PathBuf, u32, Option<String>, bool)>,
    messages: &Receiver<ReaderMessage>,
    metadata_work: &Sender<(i64, i64, PathBuf, u32)>,
) -> Result<()> {
    let mut next_id = 2i64;
    let mut in_flight = 0usize;
    let mut refill = true;
    let mut last_emit = Instant::now();
    'collect: loop {
        if job.cancel.load(Ordering::Acquire) {
            break;
        }
        if stop.load(Ordering::Acquire) {
            return Err(AppError::new("WORKER_FAILED", "枚举线程异常退出"));
        }
        collector.harvest()?;
        collector.flush_if_full()?;
        if refill && in_flight < DIRECTORY_PREFETCH {
            let queue_started = collector.profile.enabled.then(Instant::now);
            let limit = (DIRECTORY_PREFETCH - in_flight)
                .min(collector.consumed_limit - collector.batch.consumed.len());
            for _ in 0..limit {
                if collector.queue.needs_refill() {
                    let read_started = collector.profile.enabled.then(Instant::now);
                    let count = refill_visible(
                        &mut collector.queue,
                        collector.connection,
                        collector.scan_id,
                    )?;
                    if let Some(started) = read_started {
                        collector.profile.directory_queue_read_ms += milliseconds(started);
                        collector.profile.directory_queue_queries += 1;
                        collector.profile.directory_queue_read_rows += count;
                        collector.profile.directory_queue_peak_cached_entries = collector
                            .profile
                            .directory_queue_peak_cached_entries
                            .max(collector.queue.cache.len());
                        collector.profile.directory_queue_peak_cached_bytes = collector
                            .profile
                            .directory_queue_peak_cached_bytes
                            .max(collector.queue.owned_bytes());
                    }
                }
                if collector.queue.cache.is_empty() {
                    break;
                }
                #[cfg(test)]
                {
                    let hook = DISPATCH_TEST_HOOK
                        .lock()
                        .unwrap()
                        .as_ref()
                        .filter(|hook| hook.0 == _root)
                        .cloned();
                    if let Some((_, entered, release)) = hook {
                        entered.send(()).unwrap();
                        release.recv().unwrap();
                    }
                }
                if job.cancel.load(Ordering::Acquire) {
                    break 'collect;
                }
                if stop.load(Ordering::Acquire) {
                    return Err(AppError::new("WORKER_FAILED", "枚举线程异常退出"));
                }
                if collector.port.ledger_count >= collector.port.ledger_limit {
                    return Err(AppError::new("WRITER_PROTOCOL", "扫描派发记录超过内存预算"));
                }
                let marked = collector.profile.enabled.then(Instant::now);
                let (id, raw, depth, expected, deferred) = {
                    let mut mark = collector
                        .connection
                        .prepare_cached("INSERT INTO dispatched_dirs(id) VALUES(?1)")?;
                    collector.queue.consume(&mut mark)?
                };
                collector.batch.consumed.push(id);
                collector.port.ledger_count += 1;
                if let Some(started) = marked {
                    collector.profile.directory_queue_mark_ms += milliseconds(started);
                }
                let decode_started = collector.profile.enabled.then(Instant::now);
                let path = platform::decode_path(raw);
                if let Some(started) = decode_started {
                    collector.profile.directory_queue_decode_ms += milliseconds(started);
                }
                if depth >= 4096 {
                    collector.add_error(PendingScanError {
                        id,
                        path,
                        stage: "enumerate",
                        error: AppError::new("DEPTH_LIMIT", "目录深度达到 4096，未继续读取"),
                    })?;
                } else {
                    let sent = collector.profile.enabled.then(Instant::now);
                    if requests
                        .send((id, path, depth, expected, deferred))
                        .is_err()
                    {
                        if job.cancel.load(Ordering::Acquire) {
                            break 'collect;
                        }
                        return Err(AppError::new("WORKER_FAILED", "枚举线程已退出"));
                    }
                    if let Some(started) = sent {
                        collector.profile.directory_queue_send_ms += milliseconds(started);
                    }
                    in_flight += 1;
                }
            }
            if let Some(started) = queue_started {
                collector.profile.directory_queue_ms += milliseconds(started);
            }
            refill = false;
        }
        if in_flight == 0 {
            if collector.pending > 0 || !collector.batch.empty() {
                if collector.profile.enabled {
                    collector.profile.flush_drain += 1;
                }
                collector.flush()?;
            }
            if collector.port.wait(collector.connection)? {
                collector.queue.reset_after_flush();
            }
            if !collector.queue.exhausted || !collector.queue.cache.is_empty() {
                refill = true;
                continue;
            }
            // An acknowledgement can publish newer rows above the old cursor.
            if collector.queue.needs_refill() {
                refill = true;
                continue;
            }
            break;
        }
        #[cfg(test)]
        {
            let hook = READER_TEST_HOOK
                .lock()
                .unwrap()
                .as_ref()
                .filter(|hook| hook.path == _root)
                .cloned();
            if let Some(hook) = hook {
                hook.waiting.send(()).unwrap();
            }
        }
        let wait_started = collector.profile.enabled.then(Instant::now);
        let received = messages.recv_timeout(Duration::from_millis(50));
        if let Some(started) = wait_started {
            collector.profile.reader_wait_ms += milliseconds(started);
        }
        let ReaderMessage {
            message,
            mut auxiliary_permit,
        } = match received {
            Ok(message) => message,
            Err(_) if job.cancel.load(Ordering::Acquire) => break,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                refill = true;
                continue;
            }
            Err(_) => return Err(AppError::new("WORKER_FAILED", "枚举线程中断")),
        };
        #[cfg(test)]
        {
            let fault = TEST_FAULT
                .lock()
                .unwrap()
                .as_ref()
                .filter(|fault| {
                    fault.scan_id == collector.scan_id
                        && fault.data_dir == collector.service.repo.data_dir
                        && fault.coordinator
                })
                .cloned();
            if let Some(fault) = fault {
                fault.entered.send(()).unwrap();
                fault.release.recv().unwrap();
                panic!("injected coordinator failure");
            }
        }
        match message {
            DirectoryMessage::Metadata(update) => {
                collector.add_metadata(update)?;
            }
            DirectoryMessage::Error {
                parent,
                path,
                stage,
                error,
            } => {
                collector.add_error(PendingScanError {
                    id: parent,
                    path,
                    stage,
                    error,
                })?;
            }
            DirectoryMessage::Done(timings) => {
                in_flight = in_flight
                    .checked_sub(1)
                    .ok_or_else(|| AppError::new("WRITER_PROTOCOL", "目录完成通知数量不一致"))?;
                collector.profile.directory_check_ms += timings.directory_check_ms;
                collector.profile.enumerate_ms += timings.enumerate_ms;
                collector.profile.encode_ms += timings.encode_ms;
                refill = true;
            }
            DirectoryMessage::Fatal(error) => return Err(error),
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
                        break 'collect;
                    }
                    let fast = entry
                        .scan_file_metadata()
                        .or_else(|| entry.scan_directory_metadata())
                        .is_some();
                    let path = entry.path;
                    if path.starts_with(&collector.service.repo.data_dir)
                        || collector.service.is_work_file(&path)
                    {
                        continue;
                    }
                    let id = next_id;
                    next_id = next_id
                        .checked_add(1)
                        .ok_or_else(|| AppError::new("SIZE_OUT_OF_RANGE", "节点数量超出上限"))?;
                    if let Some(metadata) = entry.hint.filter(|_| fast) {
                        collector.add_row((id, parent, path, depth + 1, Ok(metadata), encoded))?;
                    } else {
                        // A dispatched path remains in the collection budget
                        // until its complete result is accepted into a packet.
                        let bytes = path.capacity();
                        if bytes > METADATA_ITEM_BYTES {
                            return Err(AppError::new(
                                "READER_PAYLOAD_BUDGET",
                                "元数据路径超过内存预算",
                            ));
                        }
                        if collector.batch.heap_bytes + collector.reserved_metadata + bytes
                            > BATCH_PATH_BYTES
                            || collector.pending == 2000
                        {
                            collector.flush()?;
                        }
                        metadata_work
                            .send((id, parent, path, depth + 1))
                            .map_err(|_| AppError::new("WORKER_FAILED", "元数据工作线程已退出"))?;
                        collector.pending += 1;
                        collector.reserved_metadata += bytes;
                    }
                    collector.flush_if_full()?;
                }
                drop(permit);
            }
        }
        drop(auxiliary_permit);
        collector.flush_if_full()?;
        if last_emit.elapsed() >= Duration::from_millis(100) {
            emit_scan(
                collector.service,
                collector.scan_id,
                job,
                "running",
                "读取目录与元数据",
            )?;
            last_emit = Instant::now();
        }
    }
    Ok(())
}

fn write_loop(
    service: &Service,
    scan_id: &str,
    job: &JobRuntime,
    mut c: rusqlite::Connection,
    commands: Receiver<WriteCommand>,
    acknowledgements: Sender<Result<CommitAck>>,
    mut profile: ScanProfile,
) -> Result<WriterResult> {
    let outcome = (|| -> Result<Scan> {
        for command in commands {
            match command {
                WriteCommand::Batch {
                    sequence,
                    mut batch,
                } => {
                    let (_, empty) = crossbeam_channel::bounded(0);
                    let result = (|| -> Result<()> {
                        {
                            let mut insert =
                                c.prepare_cached("INSERT INTO consumed_dirs(id) VALUES(?1)")?;
                            for id in &batch.consumed {
                                insert.execute([id])?;
                            }
                        }
                        flush_batch_with_errors(
                            service,
                            &mut c,
                            scan_id,
                            0,
                            &empty,
                            &mut batch.rows,
                            &mut batch.metadata,
                            &mut batch.errors,
                            &mut profile,
                        )
                    })();
                    if let Err(error) = result {
                        let _ = acknowledgements.send(Err(error.clone()));
                        return Err(error);
                    }
                    #[cfg(test)]
                    {
                        let fault = TEST_FAULT
                            .lock()
                            .unwrap()
                            .as_ref()
                            .filter(|fault| {
                                fault.scan_id == scan_id
                                    && fault.data_dir == service.repo.data_dir
                                    && fault.ack_sequence == Some(sequence)
                            })
                            .cloned();
                        if let Some(fault) = fault {
                            fault.entered.send(()).unwrap();
                            fault.release.recv().unwrap();
                        }
                    }
                    acknowledgements
                        .send(Ok(CommitAck {
                            sequence,
                            consumed: batch.consumed,
                        }))
                        .map_err(|_| AppError::new("WORKER_FAILED", "扫描协调线程已退出"))?;
                }
                WriteCommand::Finish => {
                    if !job.cancel.load(Ordering::Acquire) {
                        crate::repository::check_directory_metadata_complete(&c, scan_id)?;
                    }
                    let scan = service.repo.scan(scan_id)?;
                    let state = if job.cancel.load(Ordering::Acquire) {
                        "cancelled"
                    } else if scan.error_count != "0" {
                        "completed_with_errors"
                    } else {
                        "completed"
                    };
                    let started = Instant::now();
                    let result = {
                        let _writer = service.writer.lock().unwrap();
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            service.repo.finish(scan_id, state)
                        }))
                        .unwrap_or_else(|_| {
                            Err(AppError::new("WORKER_FAILED", "扫描终结事务异常"))
                        })?
                    };
                    profile.finish_ms = milliseconds(started);
                    return Ok(result);
                }
                WriteCommand::Abort => return Err(AppError::new("SCAN_ABORTED", "扫描已停止写入")),
            }
        }
        Err(AppError::new("WORKER_FAILED", "扫描协调线程中断"))
    })();
    let statistics = (|| -> Result<()> {
        if profile.enabled {
            for (operation, value) in [
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
                let mut high = 0;
                unsafe {
                    rusqlite::ffi::sqlite3_db_status(c.handle(), operation, value, &mut high, 0);
                }
            }
            profile.page_size = c.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        }
        Ok(())
    })();
    let close_started = Instant::now();
    let close = c.close().map_err(|(connection, error)| {
        drop(connection);
        AppError::from(error)
    });
    profile.close_ms = milliseconds(close_started);
    let scan = outcome?;
    statistics?;
    close?;
    #[cfg(test)]
    if TEST_FAULT.lock().unwrap().as_ref().is_some_and(|fault| {
        fault.scan_id == scan_id && fault.data_dir == service.repo.data_dir && fault.close_failure
    }) {
        return Err(AppError::new(
            "DATABASE_CLOSE",
            "injected closed-connection failure result",
        ));
    }
    Ok(WriterResult { scan, profile })
}

fn merge_writer_profile(target: &mut ScanProfile, source: ScanProfile) {
    target.metadata_wait_ms += source.metadata_wait_ms;
    target.writer_wait_ms += source.writer_wait_ms;
    target.insert_ms += source.insert_ms;
    target.sql_insert_ms += source.sql_insert_ms;
    target.pending_insert_ms += source.pending_insert_ms;
    target.encode_ms += source.encode_ms;
    target.directory_metadata_apply_ms += source.directory_metadata_apply_ms;
    target.directory_metadata_updates += source.directory_metadata_updates;
    target.aggregate_ms += source.aggregate_ms;
    target.aggregate_direct_parents += source.aggregate_direct_parents;
    target.aggregate_ancestor_rows += source.aggregate_ancestor_rows;
    target.aggregate_fallback_batches += source.aggregate_fallback_batches;
    target.commit_ms += source.commit_ms;
    target.writer_observation = source.writer_observation;
    target.cache_write_pages = source.cache_write_pages;
    target.cache_hit_pages = source.cache_hit_pages;
    target.cache_miss_pages = source.cache_miss_pages;
    target.cache_spill_pages = source.cache_spill_pages;
    target.cache_used_bytes = source.cache_used_bytes;
    target.page_size = source.page_size;
    target.finish_ms = source.finish_ms;
    target.close_ms = source.close_ms;
    target.batches = source.batches;
    target.max_flush_ms = source.max_flush_ms;
    target.peak_batch_entries = target.peak_batch_entries.max(source.peak_batch_entries);
    target.first_commit_ms = source.first_commit_ms;
}

pub(super) fn scan(
    service: &Service,
    scan_id: &str,
    root: &Path,
    job: &JobRuntime,
    fast: bool,
    reader_count: usize,
    output_slots: usize,
) -> Result<Scan> {
    #[cfg(test)]
    let fast = if TEST_FAULT
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|fault| fault.root.as_deref() == Some(root) && fault.legacy)
    {
        false
    } else {
        fast
    };
    #[cfg(test)]
    {
        let mut fault = TEST_FAULT.lock().unwrap();
        if let Some(fault) = fault
            .as_mut()
            .filter(|fault| fault.root.as_deref() == Some(root))
        {
            fault.scan_id = scan_id.into();
        }
    }
    let started = Instant::now();
    let consumed_limit =
        consumed_limit_override(std::env::var("SPACEFOLD_CONSUMED_LIMIT").ok().as_deref());
    let enabled = std::env::var_os("SPACEFOLD_PROFILE").is_some();
    let mut profile = ScanProfile {
        enabled,
        reader_wait_probe: enabled.then(|| Arc::new(ReaderWaitProbe::default())),
        consumed_limit,
        directory_readers: reader_count,
        output_slots,
        output_slot_storage_upper_bound_bytes: output_slots
            * (std::mem::size_of::<ReaderMessage>()
                + std::mem::size_of::<usize>()
                + std::mem::align_of::<ReaderMessage>()),
        started: Some(started),
        ..Default::default()
    };
    let mut c = service.repo.connect()?;
    c.execute_batch(
        "PRAGMA temp_store=MEMORY;CREATE TEMP TABLE consumed_dirs(id INTEGER PRIMARY KEY);",
    )?;
    let root_metadata = platform::metadata(root)?;
    if root_metadata.kind != "directory" {
        return Err(AppError::new("NOT_DIRECTORY", "扫描来源必须是目录"));
    }
    {
        let _writer = service.writer.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute("UPDATE scans SET status='running' WHERE id=?1", [scan_id])?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE scan_id=?1 AND id=1)",
            [scan_id],
            |row| row.get(0),
        )?;
        if !exists {
            insert_node(&tx, scan_id, 1, None, root, 0, &root_metadata, None)?;
        }
        tx.commit()?;
    }
    let read = service.repo.connect()?;
    read.execute_batch("PRAGMA cache_size=-1024;PRAGMA temp_store=MEMORY;CREATE TEMP TABLE dispatched_dirs(id INTEGER PRIMARY KEY);CREATE TEMP TABLE unpublished_dirs(id INTEGER PRIMARY KEY);")?;
    let directory_reader = platform::ScanDirectoryReader::new(root, fast).with_profiling(enabled);
    let reader_budget = Arc::new(ReaderBudget {
        observation: profile.reader_wait_probe.clone(),
        ..Default::default()
    });
    let auxiliary_budget = Arc::new(ReaderBudget {
        limit: AUXILIARY_MESSAGE_BYTES,
        ..Default::default()
    });
    let metadata_budget = Arc::new(ReaderBudget {
        limit: METADATA_REPLY_BYTES,
        ..Default::default()
    });
    let stop = Arc::new(AtomicBool::new(false));
    let writer_profile = ScanProfile {
        enabled,
        started: Some(started),
        reader_wait_probe: profile.reader_wait_probe.clone(),
        ..Default::default()
    };
    let result = std::thread::scope(|scope| -> Result<WriterResult> {
        let (commands, input) = crossbeam_channel::bounded(0);
        let (acknowledgements, acknowledgements_rx) = crossbeam_channel::bounded(1);
        let writer = scope.spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                write_loop(
                    service,
                    scan_id,
                    job,
                    c,
                    input,
                    acknowledgements,
                    writer_profile,
                )
            }))
            .unwrap_or_else(|_| Err(AppError::new("WORKER_FAILED", "扫描写线程异常退出")))
        });
        let mut port = WriterPort {
            commands,
            acknowledgements: acknowledgements_rx,
            sequence: 0,
            outstanding: None,
            committed_batches: 0,
            unpublished: Vec::with_capacity(BATCH_ENTRIES),
            ledger_count: 0,
            ledger_limit: 2 * consumed_limit + DIRECTORY_PREFETCH,
        };
        let (requests, request_rx) = crossbeam_channel::bounded(DIRECTORY_PREFETCH);
        let (output, messages) = crossbeam_channel::bounded(output_slots);
        #[cfg(test)]
        {
            let mut fault = TEST_FAULT.lock().unwrap();
            if let Some(fault) = fault
                .as_mut()
                .filter(|fault| fault.scan_id == scan_id && fault.data_dir == service.repo.data_dir)
            {
                fault.output = Some(output.clone());
                fault.budgets = Some((
                    reader_budget.clone(),
                    auxiliary_budget.clone(),
                    metadata_budget.clone(),
                ));
            }
        }
        let mut readers = Vec::with_capacity(reader_count);
        for _ in 0..reader_count {
            let requests = request_rx.clone();
            let sender = output.clone();
            let reader_budget = reader_budget.clone();
            let auxiliary_budget = auxiliary_budget.clone();
            let stop = stop.clone();
            let directory_reader = directory_reader.clone();
            readers.push(scope.spawn(move || -> Result<()> {
                let fault = sender.clone();
                let output = ReaderOutput {
                    sender,
                    observation: reader_budget.observation.clone(),
                    auxiliary_budget: auxiliary_budget.clone(),
                    cancel: &job.cancel,
                };
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    read_directories(
                        requests,
                        output,
                        &job.cancel,
                        &stop,
                        enabled,
                        reader_budget.clone(),
                        directory_reader,
                    )
                }))
                .is_err()
                {
                    stop.store(true, Ordering::Release);
                    reader_budget.stop();
                    auxiliary_budget.stop();
                    let error = AppError::new("WORKER_FAILED", "枚举线程异常退出");
                    let _ = fault.send(ReaderMessage {
                        message: DirectoryMessage::Fatal(error.clone()),
                        auxiliary_permit: None,
                    });
                    return Err(error);
                }
                Ok(())
            }));
        }
        drop(request_rx);
        drop(output);
        let (metadata_work, metadata_work_rx) = crossbeam_channel::bounded(2000);
        let (metadata_output, metadata_rx) = crossbeam_channel::bounded(2000);
        let workers =
            spawn_metadata_workers(metadata_work_rx, metadata_output, metadata_budget.clone());
        // This connection never writes the persistent schema and never keeps a
        // read transaction across a channel receive. Its TEMP ledger is finite.
        let mut collector = Collector {
            service,
            scan_id,
            connection: &read,
            port: &mut port,
            profile: &mut profile,
            batch: WriteBatch::new(consumed_limit),
            queue: DirectoryQueueCursor::default(),
            metadata: &metadata_rx,
            pending: 0,
            reserved_metadata: 0,
            consumed_limit,
        };
        let mut outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            collect_directories(
                &mut collector,
                root,
                job,
                &stop,
                &requests,
                &messages,
                &metadata_work,
            )
        }))
        .unwrap_or_else(|_| Err(AppError::new("WORKER_FAILED", "扫描协调线程异常退出")));
        // Disconnect before stopping budgets/joining. Both an output sender and
        // a permit waiter must have an unconditional path out of reader shutdown.
        drop(requests);
        drop(messages);
        stop.store(true, Ordering::Release);
        reader_budget.stop();
        auxiliary_budget.stop();
        #[cfg(test)]
        {
            let hook = DEFERRED_DIRECTORY_HOOK
                .lock()
                .unwrap()
                .as_ref()
                .filter(|hook| hook.0.parent() == Some(root))
                .cloned();
            if let Some((_, _, _, stopped)) = hook {
                let _ = stopped.try_send(());
            }
        }
        for reader in readers {
            match reader.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => outcome = Err(error),
                Err(_) => outcome = Err(AppError::new("WORKER_FAILED", "枚举线程异常退出")),
            }
        }
        if outcome.is_ok() {
            // Cancellation retains all accepted metadata work. The separate
            // reply budget remains active while these results are drained.
            outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                collector
                    .flush()
                    .and_then(|_| collector.port.wait(collector.connection).map(|_| ()))
            }))
            .unwrap_or_else(|_| Err(AppError::new("WORKER_FAILED", "扫描协调线程异常退出")));
        }
        drop(collector);
        drop(metadata_work);
        drop(metadata_rx);
        metadata_budget.stop();
        for worker in workers {
            if worker.join().is_err() {
                outcome = Err(AppError::new("WORKER_FAILED", "元数据线程异常退出"));
            }
        }
        // Close the coordinator first so the writer's final close remains the
        // final SQLite-resource boundary, including the natural WAL checkpoint.
        if let Err((connection, error)) = read.close() {
            drop(connection);
            outcome = Err(error.into());
        }
        let command = if outcome.is_ok() {
            WriteCommand::Finish
        } else {
            WriteCommand::Abort
        };
        let sent = port.commands.send(command);
        drop(port);
        let written = writer
            .join()
            .unwrap_or_else(|_| Err(AppError::new("WORKER_FAILED", "扫描写线程异常退出")));
        outcome?;
        sent.map_err(|_| AppError::new("WORKER_FAILED", "扫描写线程已退出"))?;
        written
    })?;
    if enabled {
        profile.directory_api = directory_reader.api_profile();
        let auxiliary = auxiliary_budget.state.lock().unwrap();
        profile.auxiliary_message_peak_bytes = auxiliary.2;
        profile.auxiliary_message_budget_waits = auxiliary.3;
        if let Some(probe) = &profile.reader_wait_probe {
            profile.reader_backpressure = probe.snapshot();
        }
        merge_writer_profile(&mut profile, result.profile);
        eprintln!(
            "{}",
            serde_json::json!({ "scanProfile": profile, "totalMs": milliseconds(started), "writerMode": "overlap" })
        );
    }
    let state = &result.scan.status;
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
    Ok(result.scan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct ClearFault;
    impl Drop for ClearFault {
        fn drop(&mut self) {
            *TEST_FAULT.lock().unwrap() = None;
        }
    }
    fn fault_control() -> std::sync::MutexGuard<'static, ()> {
        // ClearFault restores the hook before this test-only guard unwinds.
        // Recovering this unit lock isolates a failed assertion from other tests.
        TEST_FAULT_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
    fn setup() -> (tempfile::TempDir, Arc<Service>, PathBuf, String) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("source");
        fs::create_dir_all(&root).unwrap();
        let root = platform::absolute(&root).unwrap();
        let service = Service::new(&temp.path().join("data")).unwrap();
        let scan = service.repo.new_scan_id().unwrap();
        service.repo.insert_scan(&scan, &root, false).unwrap();
        let mut c = service.repo.connect().unwrap();
        let tx = c.transaction().unwrap();
        insert_node(
            &tx,
            &scan,
            1,
            None,
            &root,
            0,
            &platform::metadata(&root).unwrap(),
            None,
        )
        .unwrap();
        tx.commit().unwrap();
        (temp, service, root, scan)
    }
    fn read_connection(service: &Service) -> rusqlite::Connection {
        let c = service.repo.connect().unwrap();
        c.execute_batch("CREATE TEMP TABLE dispatched_dirs(id INTEGER PRIMARY KEY);CREATE TEMP TABLE unpublished_dirs(id INTEGER PRIMARY KEY);").unwrap();
        c
    }
    fn row(id: i64, root: &Path, name: &str) -> MetadataResult {
        let path = root.join(name);
        fs::create_dir(&path).unwrap();
        let metadata = platform::metadata(&path).unwrap();
        (id, 1, path, 1, Ok(metadata), None)
    }
    fn port(commands: Sender<WriteCommand>, ack: Receiver<Result<CommitAck>>) -> WriterPort {
        WriterPort {
            commands,
            acknowledgements: ack,
            sequence: 0,
            outstanding: None,
            committed_batches: 0,
            unpublished: Vec::with_capacity(BATCH_ENTRIES),
            ledger_count: 0,
            ledger_limit: 2 * DEFAULT_CONSUMED_LIMIT + DIRECTORY_PREFETCH,
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
                started.elapsed() < Duration::from_secs(5),
                "terminal event not delivered"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn install_fault(
        service: &Service,
        root: Option<PathBuf>,
        id: String,
        before_commit: bool,
        coordinator: bool,
        ack_sequence: Option<u64>,
    ) -> (Receiver<()>, Sender<()>) {
        let (entered, entered_rx) = crossbeam_channel::bounded(1);
        let (release, release_rx) = crossbeam_channel::bounded(1);
        *TEST_FAULT.lock().unwrap() = Some(TestFault {
            scan_id: id,
            data_dir: service.repo.data_dir.clone(),
            root,
            before_commit,
            coordinator,
            ack_sequence,
            entered,
            release: release_rx,
            output: None,
            budgets: None,
            close_failure: false,
            legacy: false,
            metadata_oversize: false,
        });
        (entered_rx, release)
    }
    fn release_observer() -> Option<(Arc<ReaderBudget>, Arc<ReaderBudget>, Arc<ReaderBudget>)> {
        let mut fault = TEST_FAULT.lock().unwrap();
        let fault = fault.as_mut().unwrap();
        fault.output.take();
        fault.budgets.clone()
    }
    fn used(budget: &ReaderBudget) -> usize {
        budget.state.lock().unwrap().0
    }

    #[test]
    fn exact_publication_excludes_low_id_after_commit_until_its_acknowledgement() {
        let _control = fault_control();
        let (_temp, service, root, id) = setup();
        let read = read_connection(&service);
        let c = service.repo.connect().unwrap();
        c.execute_batch("CREATE TEMP TABLE consumed_dirs(id INTEGER PRIMARY KEY);")
            .unwrap();
        let job = JobRuntime::new(JobEvent::new(&id, &id, "scan", 1), None);
        let (entered, release) = install_fault(&service, None, id.clone(), false, false, Some(2));
        let _clear = ClearFault;
        std::thread::scope(|scope| {
            let (commands, input) = crossbeam_channel::bounded(0);
            let (ack, output) = crossbeam_channel::bounded(1);
            let writer = scope
                .spawn(|| write_loop(&service, &id, &job, c, input, ack, ScanProfile::default()));
            let mut port = port(commands, output);
            let mut batch = WriteBatch::new(DEFAULT_CONSUMED_LIMIT);
            batch.rows.push(row(1000, &root, "high"));
            port.submit(&read, &mut batch, DEFAULT_CONSUMED_LIMIT)
                .unwrap();
            port.wait(&read).unwrap();
            batch.rows.push(row(2, &root, "low-returned-late"));
            port.submit(&read, &mut batch, DEFAULT_CONSUMED_LIMIT)
                .unwrap();
            entered.recv_timeout(Duration::from_secs(3)).unwrap();
            let committed: bool = read
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM pending_dirs WHERE scan_id=?1 AND id=2)",
                    [&id],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(committed);
            let mut visible = DirectoryQueueCursor::default();
            refill_visible(&mut visible, &read, &id).unwrap();
            assert!(!visible.cache.iter().any(|item| item.0 == 2));
            assert!(visible.cache.iter().any(|item| item.0 == 1000));
            release.send(()).unwrap();
            port.wait(&read).unwrap();
            visible.reset_after_flush();
            refill_visible(&mut visible, &read, &id).unwrap();
            assert!(visible.cache.iter().any(|item| item.0 == 2));
            drop(read);
            port.commands.send(WriteCommand::Abort).unwrap();
            drop(port);
            let Err(error) = writer.join().unwrap() else {
                panic!("aborted writer reported success");
            };
            assert_eq!(error.code, "SCAN_ABORTED");
        });
    }

    #[test]
    fn failed_or_wrong_sequence_acknowledgements_never_clear_next_or_isolated_ids() {
        let (_temp, service, root, id) = setup();
        let read = read_connection(&service);
        let (commands, input) = crossbeam_channel::bounded(0);
        let (_, output) = crossbeam_channel::bounded(1);
        let mut port = port(commands, output);
        read.execute_batch(
            "INSERT INTO dispatched_dirs VALUES(1),(44);INSERT INTO unpublished_dirs VALUES(2);",
        )
        .unwrap();
        port.outstanding = Some(1);
        port.ledger_count = 2;
        assert_eq!(
            port.apply_ack(
                &read,
                CommitAck {
                    sequence: 2,
                    consumed: vec![1]
                }
            )
            .unwrap_err()
            .code,
            "WRITER_PROTOCOL"
        );
        assert_eq!(
            read.query_row("SELECT COUNT(*) FROM unpublished_dirs", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        port.apply_ack(
            &read,
            CommitAck {
                sequence: 1,
                consumed: vec![1],
            },
        )
        .unwrap();
        assert_eq!(
            read.query_row("SELECT id FROM dispatched_dirs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            44
        );
        assert_eq!(port.ledger_count, 1);
        drop(input);
        let mut batch = WriteBatch::new(DEFAULT_CONSUMED_LIMIT);
        batch.rows.push(row(2, &root, "isolated"));
        assert_eq!(
            port.submit(&read, &mut batch, DEFAULT_CONSUMED_LIMIT)
                .unwrap_err()
                .code,
            "WORKER_FAILED"
        );
        assert_eq!(
            read.query_row("SELECT id FROM unpublished_dirs", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert!(service.repo.node(&id, "2", None).is_err());
    }

    #[test]
    fn writer_transaction_panic_rolls_back_without_poison_and_delivers_real_failed_event() {
        let _control = fault_control();
        let (_temp, service, root, _) = setup();
        for n in 0..100 {
            fs::write(root.join(format!("file-{n}")), b"abc").unwrap();
        }
        let (entered, release) = install_fault(
            &service,
            Some(root.clone()),
            String::new(),
            true,
            false,
            None,
        );
        let _clear = ClearFault;
        let scan = service.start_scan(root.clone(), None).unwrap();
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        release_observer();
        release.send(()).unwrap();
        let event = terminal(&service, &scan.scan_id);
        assert_eq!(event.status, "failed");
        assert_eq!(event.error.as_ref().unwrap().code, "WORKER_FAILED");
        assert!(!service.writer.is_poisoned());
        assert_eq!(
            service.repo.scan(&scan.scan_id).unwrap().discovered_count,
            "1"
        );
        assert_eq!(
            service
                .repo
                .node(&scan.scan_id, "1", None)
                .unwrap()
                .node
                .logical_total_bytes,
            "0"
        );
        *TEST_FAULT.lock().unwrap() = None;
        let next = service.start_scan(root, None).unwrap();
        assert_eq!(terminal(&service, &next.scan_id).status, "completed");
    }

    #[test]
    fn coordinator_panic_disconnects_a_full_output_queue_and_joins_all_budget_owners() {
        let _control = fault_control();
        let (_temp, service, root, _) = setup();
        for n in 0..10000 {
            fs::write(root.join(format!("file-{n}")), b"").unwrap();
        }
        let (entered, release) = install_fault(
            &service,
            Some(root.clone()),
            String::new(),
            false,
            true,
            None,
        );
        let _clear = ClearFault;
        TEST_FAULT.lock().unwrap().as_mut().unwrap().legacy = true;
        let scan = service.start_scan(root, None).unwrap();
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        let waiting = Instant::now();
        loop {
            let full = TEST_FAULT
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .output
                .as_ref()
                .unwrap()
                .is_full();
            if full {
                break;
            }
            assert!(
                waiting.elapsed() < Duration::from_secs(3),
                "reader did not reach output backpressure"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let budgets = release_observer().unwrap();
        release.send(()).unwrap();
        let event = terminal(&service, &scan.scan_id);
        assert_eq!(event.status, "failed");
        assert_eq!(event.error.as_ref().unwrap().code, "WORKER_FAILED");
        assert_eq!(used(&budgets.0), 0);
        assert_eq!(used(&budgets.1), 0);
        assert_eq!(used(&budgets.2), 0);
        assert!(!service.writer.is_poisoned());
    }

    #[test]
    fn cancellation_waits_for_accepted_commit_and_closes_before_terminal_with_exact_partial_counts()
    {
        let _control = fault_control();
        let (_temp, service, root, _) = setup();
        for n in 0..1000 {
            fs::write(root.join(format!("file-{n}")), b"xyz").unwrap();
        }
        for n in 0..100 {
            fs::create_dir(root.join(format!("directory-{n}"))).unwrap();
        }
        let (entered, release) = install_fault(
            &service,
            Some(root.clone()),
            String::new(),
            false,
            false,
            Some(1),
        );
        let _clear = ClearFault;
        TEST_FAULT.lock().unwrap().as_mut().unwrap().legacy = true;
        let scan = service.start_scan(root, None).unwrap();
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        service.cancel_job(&scan.scan_id).unwrap();
        assert!(!service.job(&scan.scan_id).unwrap().terminal());
        let budgets = release_observer().unwrap();
        release.send(()).unwrap();
        let event = terminal(&service, &scan.scan_id);
        assert_eq!(event.status, "cancelled");
        let saved = service.repo.scan(&scan.scan_id).unwrap();
        let count: i64 = saved.discovered_count.parse().unwrap();
        assert!(count > 1 && count <= 1101);
        let root = service.repo.node(&scan.scan_id, "1", None).unwrap().node;
        let files: i64 = root.file_count.parse().unwrap();
        assert_eq!(root.logical_total_bytes, (files * 3).to_string());
        assert!(root.directory_count.parse::<i64>().unwrap() > 0);
        assert_eq!(root.children_count, (count - 1).to_string());
        assert!(root.incomplete);
        assert_eq!(used(&budgets.0), 0);
        assert_eq!(used(&budgets.1), 0);
        assert_eq!(used(&budgets.2), 0);
        let db = service.repo.connect().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM nodes WHERE scan_id=?1",
                [&scan.scan_id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            count
        );
    }

    #[test]
    fn metadata_reply_real_four_mib_limit_retains_accepted_results_and_oversized_item_is_fatal() {
        let (_temp, _service, root, _) = setup();
        let (work, input) = crossbeam_channel::bounded(2000);
        let (output, replies) = crossbeam_channel::bounded(2000);
        let budget = Arc::new(ReaderBudget {
            limit: METADATA_REPLY_BYTES,
            ..Default::default()
        });
        let workers = spawn_metadata_workers(input, output, budget.clone());
        let mut accepted_path_bytes = 0;
        for id in 2..34 {
            let mut path = root.join(format!("reply-{id}"));
            fs::write(&path, b"x").unwrap();
            path.reserve_exact(128 * 1024 - path.as_os_str().len());
            accepted_path_bytes += path.capacity();
            work.send((id, 1, path, 1)).unwrap();
        }
        assert!(accepted_path_bytes <= BATCH_PATH_BYTES);
        let started = Instant::now();
        while budget.state.lock().unwrap().3 == 0 {
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "metadata replies did not reach the real four MiB quota"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        // Accepted work uses its own shutdown budget, independent of a scan
        // cancel flag and the stopped filesystem-reader budgets.
        let mut ids = std::collections::BTreeSet::new();
        for _ in 0..32 {
            let MetadataMessage::Complete(reply) =
                replies.recv_timeout(Duration::from_secs(3)).unwrap()
            else {
                panic!("valid metadata result failed")
            };
            assert!(reply.row.4.is_ok());
            ids.insert(reply.row.0);
            drop(reply);
        }
        assert_eq!(ids.len(), 32);
        let mut path = root.join("oversized-capacity");
        fs::write(&path, b"kept").unwrap();
        path.reserve_exact(METADATA_ITEM_BYTES + 1);
        work.send((34, 1, path, 1)).unwrap();
        let MetadataMessage::Fatal(error) = replies.recv_timeout(Duration::from_secs(3)).unwrap()
        else {
            panic!("oversized result silently became a node")
        };
        assert_eq!(error.code, "READER_PAYLOAD_BUDGET");
        drop(work);
        drop(replies);
        budget.stop();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(used(&budget), 0);
        assert_eq!(fs::read(root.join("oversized-capacity")).unwrap(), b"kept");
    }

    #[test]
    fn oversized_worker_reply_delivers_a_real_failed_event_instead_of_an_unknown_node() {
        let _control = fault_control();
        let (_temp, service, root, _) = setup();
        fs::create_dir(root.join("accepted-directory")).unwrap();
        fs::write(root.join("accepted-directory/inside-file"), b"preserved").unwrap();
        let (_entered, _release) = install_fault(
            &service,
            Some(root.clone()),
            String::new(),
            false,
            false,
            None,
        );
        let _clear = ClearFault;
        {
            let mut fault = TEST_FAULT.lock().unwrap();
            let fault = fault.as_mut().unwrap();
            fault.legacy = true;
            fault.metadata_oversize = true;
        }
        let scan = service.start_scan(root.clone(), None).unwrap();
        let event = terminal(&service, &scan.scan_id);
        assert_eq!(event.status, "failed");
        assert_eq!(event.error.as_ref().unwrap().code, "READER_PAYLOAD_BUDGET");
        assert_eq!(
            service.repo.scan(&scan.scan_id).unwrap().discovered_count,
            "1"
        );
        assert_eq!(
            fs::read(root.join("accepted-directory/inside-file")).unwrap(),
            b"preserved"
        );
        let budgets = release_observer().unwrap();
        assert_eq!(used(&budgets.0), 0);
        assert_eq!(used(&budgets.1), 0);
        assert_eq!(used(&budgets.2), 0);
    }

    #[test]
    fn finish_and_closed_connection_failure_never_emit_success_and_allow_failed_finalization() {
        let _control = fault_control();
        for close_failure in [false, true] {
            let (_temp, service, root, _) = setup();
            fs::write(root.join("accepted-file"), b"xyz").unwrap();
            let (_entered, _release) = install_fault(
                &service,
                Some(root.clone()),
                String::new(),
                false,
                false,
                None,
            );
            let _clear = ClearFault;
            TEST_FAULT.lock().unwrap().as_mut().unwrap().close_failure = close_failure;
            if !close_failure {
                service
                    .repo
                    .connect()
                    .unwrap()
                    .execute_batch(
                        "CREATE TRIGGER deny_completed BEFORE UPDATE OF status ON scans
                     WHEN NEW.status IN ('completed','completed_with_errors')
                     BEGIN SELECT RAISE(FAIL,'injected finish transaction failure'); END;",
                    )
                    .unwrap();
            }
            let statuses = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
            let sink = statuses.clone();
            let callback: crate::service::ProgressCallback = Arc::new(move |event| {
                sink.lock().unwrap().push(event.status);
            });
            let scan = service.start_scan(root, Some(callback)).unwrap();
            let event = terminal(&service, &scan.scan_id);
            assert_eq!(event.status, "failed");
            assert_eq!(
                event.error.as_ref().unwrap().code,
                if close_failure {
                    "DATABASE_CLOSE"
                } else {
                    "DATABASE_ERROR"
                }
            );
            assert_eq!(service.repo.scan(&scan.scan_id).unwrap().status, "failed");
            let observed = statuses.lock().unwrap().clone();
            assert!(!observed
                .iter()
                .any(|status| status == "completed" || status == "completed_with_errors"));
            assert!(!service.writer.is_poisoned());
            let saved = service.repo.node(&scan.scan_id, "1", None).unwrap().node;
            assert_eq!(saved.logical_total_bytes, "3");
            assert!(saved.incomplete);
            let budgets = release_observer().unwrap();
            assert_eq!(used(&budgets.0), 0);
            assert_eq!(used(&budgets.1), 0);
            assert_eq!(used(&budgets.2), 0);
        }
    }
}
