use crate::{
    model::{AppError, Result},
    repository::Repository,
};
use rusqlite::Connection;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    Off,
    Passive,
}
impl Mode {
    pub(super) fn from_override(value: Option<&str>) -> Self {
        if value == Some("passive") {
            Self::Passive
        } else {
            Self::Off
        }
    }
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Passive => "passive",
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Stats {
    enabled: bool,
    requests: u64,
    coalesced_requests: u64,
    runs: u64,
    busy_results: u64,
    observations: u64,
    unavailable_observations: u64,
    max_log_frames: u64,
    max_backlog_frames: u64,
    pressure_waits: u64,
    restart_opportunities: u64,
    pressure_wait_ms: f64,
    helper_ms: f64,
    max_helper_ms: f64,
    shutdown_ms: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Observation {
    log: u64,
    backfilled: u64,
}
impl Observation {
    fn from_values(busy: i64, log: i64, backfilled: i64) -> Option<Self> {
        if busy != 0 || log < 0 || backfilled < 0 || backfilled > log {
            None
        } else {
            Some(Self {
                log: log as u64,
                backfilled: backfilled as u64,
            })
        }
    }
    fn backlog(self) -> u64 {
        self.log - self.backfilled
    }
}

#[derive(Clone)]
struct Settings {
    trigger_bytes: u64,
    pressure_bytes: u64,
    stall_timeout: Duration,
    #[cfg(test)]
    test: TestSettings,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            trigger_bytes: 16 * 1024 * 1024,
            pressure_bytes: 64 * 1024 * 1024,
            stall_timeout: Duration::from_secs(5),
            #[cfg(test)]
            test: TestSettings::default(),
        }
    }
}
#[derive(Default)]
struct State {
    pending: bool,
    stopping: bool,
    error: Option<AppError>,
    generation: u64,
    stats: Stats,
}
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}
fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(super) struct CheckpointHelper {
    shared: Option<Arc<Shared>>,
    worker: Option<JoinHandle<()>>,
    frame_bytes: u64,
    settings: Settings,
    profile: bool,
    shutdown_result: Option<Result<Stats>>,
}
impl CheckpointHelper {
    pub(super) fn start(repo: &Repository, root: &Path, mode: Mode, profile: bool) -> Result<Self> {
        let mut settings = Settings::default();
        #[cfg(test)]
        let mode = {
            if let Some(test) = test_settings(root) {
                if let Some(value) = test.trigger_bytes {
                    settings.trigger_bytes = value;
                }
                if let Some(value) = test.pressure_bytes {
                    settings.pressure_bytes = value;
                }
                if let Some(value) = test.stall_timeout {
                    settings.stall_timeout = value;
                }
                let forced = test.force_passive;
                settings.test = test;
                if forced {
                    Mode::Passive
                } else {
                    mode
                }
            } else {
                mode
            }
        };
        #[cfg(not(test))]
        let _ = &mut settings;
        if mode == Mode::Off {
            return Ok(Self {
                shared: None,
                worker: None,
                frame_bytes: 0,
                settings,
                profile,
                shutdown_result: None,
            });
        }
        let connection = repo.connect()?;
        let initialized = (|| -> Result<u64> {
            #[cfg(test)]
            if settings.test.fault == Fault::StartSql {
                connection.execute_batch("SELECT * FROM checkpoint_missing_table")?;
            }
            connection.execute_batch("PRAGMA cache_size=-1024;")?;
            let page_size: i64 = connection.query_row("PRAGMA page_size", [], |r| r.get(0))?;
            let page_size = u64::try_from(page_size)
                .ok()
                .filter(|size| *size > 0)
                .ok_or_else(|| AppError::new("DATABASE_ERROR", "WAL 页大小无效"))?;
            page_size
                .checked_add(24)
                .ok_or_else(|| AppError::new("DATABASE_ERROR", "WAL 页大小无效"))
        })();
        let frame_bytes = match initialized {
            Ok(value) => value,
            Err(error) => {
                return Err(close_with_error(connection, Some(error)).unwrap_err());
            }
        };
        let shared = Arc::new(Shared::default());
        lock(&shared.state).stats.enabled = true;
        // Retain the connection outside the spawn closure until the worker takes it.
        // A failed spawn therefore still reaches an explicit close.
        let owner = Arc::new(Mutex::new(Some(connection)));
        let worker_owner = owner.clone();
        let worker_shared = shared.clone();
        let worker_settings = settings.clone();
        let worker = super::spawn_worker(root, "checkpoint", 0, move || {
            let Some(connection) = lock(&worker_owner).take() else {
                return;
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                worker_loop(&connection, &worker_shared, &worker_settings, profile)
            }))
            .unwrap_or_else(|_| Err(AppError::new("CHECKPOINT_WORKER_FAILED", "检查点线程异常")));
            let error = result.err();
            #[cfg(test)]
            let error = if worker_settings.test.fault == Fault::Close && error.is_none() {
                Some(AppError::new(
                    "DATABASE_CLOSE",
                    "injected checkpoint close error",
                ))
            } else {
                error
            };
            let closed = close_with_error(connection, error);
            let mut state = lock(&worker_shared.state);
            state.error = closed.err();
            state.generation = state.generation.wrapping_add(1);
            worker_shared.changed.notify_all();
        });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                let original = super::thread_start_error(error);
                if let Some(connection) = lock(&owner).take() {
                    return Err(close_with_error(connection, Some(original)).unwrap_err());
                }
                return Err(original);
            }
        };
        Ok(Self {
            shared: Some(shared),
            worker: Some(worker),
            frame_bytes,
            settings,
            profile,
            shutdown_result: None,
        })
    }

    fn observation(&self, writer: &Connection) -> Result<Option<Observation>> {
        #[cfg(test)]
        {
            if self.settings.test.fault == Fault::NoopSql {
                writer.execute_batch("SELECT * FROM checkpoint_missing_table")?;
            }
            if self.settings.test.unknown_observation {
                return Ok(None);
            }
            if self.settings.test.alternating_unknown
                && self
                    .shared
                    .as_ref()
                    .is_some_and(|s| lock(&s.state).stats.observations % 2 == 0)
            {
                return Ok(None);
            }
        }
        observe(writer)
    }
    fn record(&self, observation: Option<Observation>) {
        let Some(shared) = &self.shared else {
            return;
        };
        let mut state = lock(&shared.state);
        state.stats.observations += 1;
        match observation {
            Some(value) => {
                state.stats.max_log_frames = state.stats.max_log_frames.max(value.log);
                state.stats.max_backlog_frames =
                    state.stats.max_backlog_frames.max(value.backlog());
            }
            None => state.stats.unavailable_observations += 1,
        }
    }
    fn pressure(&self, observation: Option<Observation>) -> bool {
        observation.is_none_or(|value| {
            value.backlog() != 0
                && value.log.saturating_mul(self.frame_bytes) >= self.settings.pressure_bytes
        })
    }
    fn error(&self) -> Option<AppError> {
        self.shared
            .as_ref()
            .and_then(|s| lock(&s.state).error.clone())
    }
    fn notify(&self) {
        let Some(shared) = &self.shared else {
            return;
        };
        let mut state = lock(&shared.state);
        if state.stopping || state.error.is_some() {
            return;
        }
        state.stats.requests += 1;
        if state.pending {
            state.stats.coalesced_requests += 1;
        }
        state.pending = true;
        shared.changed.notify_one();
    }

    pub(super) fn after_commit(&mut self, writer: &Connection) -> Result<()> {
        if self.shared.is_none() || self.shutdown_result.is_some() {
            return Ok(());
        }
        if let Some(error) = self.error() {
            return Err(error);
        }
        let observation = self.observation(writer)?;
        self.record(observation);
        if observation.is_none_or(|value| {
            value.backlog().saturating_mul(self.frame_bytes) >= self.settings.trigger_bytes
        }) {
            self.notify();
        }
        Ok(())
    }

    pub(super) fn before_batch(&mut self, writer: &Connection, cancel: &AtomicBool) -> Result<()> {
        if self.shared.is_none() || self.shutdown_result.is_some() {
            return Ok(());
        }
        if cancel.load(Ordering::Acquire) {
            self.shutdown()?;
            return Ok(());
        }
        if let Some(error) = self.error() {
            return Err(error);
        }
        let mut observation = self.observation(writer)?;
        self.record(observation);
        if !self.pressure(observation) {
            self.record_restart(observation);
            return Ok(());
        }
        let shared = self.shared.as_ref().unwrap().clone();
        lock(&shared.state).stats.pressure_waits += 1;
        let wait_started = self.profile.then(Instant::now);
        let result = (|| -> Result<()> {
            let mut last_progress = Instant::now();
            let mut last_known = observation;
            loop {
                if cancel.load(Ordering::Acquire) {
                    self.shutdown()?;
                    return Ok(());
                }
                if let Some(error) = self.error() {
                    return Err(error);
                }
                if !self.pressure(observation) {
                    self.record_restart(observation);
                    return Ok(());
                }
                if last_progress.elapsed() >= self.settings.stall_timeout {
                    return Err(AppError::new(
                        "CHECKPOINT_STALLED",
                        "检查点长时间无法推进；已提交的扫描结果仍保留",
                    ));
                }
                let retry_started = Instant::now();
                self.notify();
                let state = lock(&shared.state);
                let generation = state.generation;
                let duration = Duration::from_millis(100).min(
                    self.settings
                        .stall_timeout
                        .saturating_sub(last_progress.elapsed()),
                );
                let _state = shared
                    .changed
                    .wait_timeout_while(state, duration, |s| {
                        s.generation == generation && s.error.is_none()
                    })
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                drop(_state);
                if let Some(error) = self.error() {
                    return Err(error);
                }
                let next = self.observation(writer)?;
                self.record(next);
                let progressed = progress(last_known, next);
                if progressed {
                    last_progress = Instant::now();
                }
                if next.is_some() {
                    last_known = next;
                }
                observation = next;
                // A pinned reader or a competing checkpoint can return immediately.
                // Retry a no-progress result at most ten times per second.
                if self.pressure(next) && !progressed {
                    let remaining =
                        Duration::from_millis(100).saturating_sub(retry_started.elapsed());
                    let state = lock(&shared.state);
                    let waited = shared
                        .changed
                        .wait_timeout_while(state, remaining, |s| s.error.is_none())
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    drop(waited);
                }
            }
        })();
        if let Some(started) = wait_started {
            lock(&shared.state).stats.pressure_wait_ms += super::milliseconds(started);
        }
        result
    }
    fn record_restart(&self, observation: Option<Observation>) {
        if observation.is_some_and(|o| {
            o.backlog() == 0
                && o.log.saturating_mul(self.frame_bytes) >= self.settings.pressure_bytes
        }) {
            if let Some(shared) = &self.shared {
                lock(&shared.state).stats.restart_opportunities += 1;
            }
        }
    }

    pub(super) fn shutdown(&mut self) -> Result<Stats> {
        if let Some(result) = &self.shutdown_result {
            return result.clone();
        }
        let Some(shared) = &self.shared else {
            self.shutdown_result = Some(Ok(Stats::default()));
            return Ok(Stats::default());
        };
        let started = self.profile.then(Instant::now);
        {
            let mut state = lock(&shared.state);
            state.stopping = true;
            state.pending = false;
            shared.changed.notify_all();
        }
        let joined = self.worker.take().map(|worker| worker.join());
        let mut state = lock(&shared.state);
        if joined.is_some_and(|result| result.is_err()) && state.error.is_none() {
            state.error = Some(AppError::new(
                "CHECKPOINT_WORKER_FAILED",
                "检查点线程未正常退出",
            ));
        }
        if let Some(started) = started {
            state.stats.shutdown_ms += super::milliseconds(started);
        }
        let result = match &state.error {
            Some(error) => Err(error.clone()),
            None => Ok(state.stats.clone()),
        };
        self.shutdown_result = Some(result.clone());
        result
    }
}
impl Drop for CheckpointHelper {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn progress(before: Option<Observation>, after: Option<Observation>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => {
            after.log < before.log || after.backfilled > before.backfilled
        }
        _ => false,
    }
}
fn observe(connection: &Connection) -> Result<Option<Observation>> {
    let row = connection.query_row("PRAGMA main.wal_checkpoint(NOOP)", [], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    });
    match row {
        Ok((busy, log, backfilled)) => Ok(Observation::from_values(busy, log, backfilled)),
        Err(error) if is_busy(&error) => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn is_busy(error: &rusqlite::Error) -> bool {
    matches!(error, rusqlite::Error::SqliteFailure(code, _) if matches!(code.code,
        rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked))
}
fn close_with_error(connection: Connection, original: Option<AppError>) -> Result<()> {
    let closed = connection.close().map_err(|(connection, error)| {
        drop(connection);
        AppError::from(error)
    });
    match (original, closed) {
        (Some(mut error), Err(close)) => {
            error.message.push_str(&format!("；关闭连接失败：{close}"));
            Err(error)
        }
        (Some(error), _) => Err(error),
        (None, result) => result,
    }
}
fn worker_loop(
    connection: &Connection,
    shared: &Shared,
    settings: &Settings,
    profile: bool,
) -> Result<()> {
    loop {
        {
            let state = lock(&shared.state);
            let mut state = shared
                .changed
                .wait_while(state, |s| !s.pending && !s.stopping)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.stopping {
                return Ok(());
            }
            state.pending = false;
        }
        #[cfg(test)]
        {
            if let Some(entered) = &settings.test.entered {
                let _ = entered.send(());
            }
            if let Some(release) = &settings.test.release {
                let _ = release.recv();
            }
            if settings.test.fault == Fault::Panic {
                panic!("injected checkpoint panic");
            }
            if settings.test.fault == Fault::Sql {
                connection.execute_batch("SELECT * FROM checkpoint_missing_table")?;
            }
        }
        #[cfg(not(test))]
        let _ = settings;
        let started = profile.then(Instant::now);
        let row = connection.query_row("PRAGMA main.wal_checkpoint(PASSIVE)", [], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        });
        let busy = match row {
            Ok((busy, _, _)) => busy != 0,
            Err(error) if is_busy(&error) => true,
            Err(error) => return Err(error.into()),
        };
        let mut state = lock(&shared.state);
        state.stats.runs += 1;
        if busy {
            state.stats.busy_results += 1;
        }
        if let Some(started) = started {
            let elapsed = super::milliseconds(started);
            state.stats.helper_ms += elapsed;
            state.stats.max_helper_ms = state.stats.max_helper_ms.max(elapsed);
        }
        state.generation = state.generation.wrapping_add(1);
        shared.changed.notify_all();
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Fault {
    #[default]
    None,
    StartSql,
    NoopSql,
    Sql,
    Panic,
    Close,
}
#[cfg(test)]
#[derive(Clone, Default)]
pub(super) struct TestSettings {
    pub force_passive: bool,
    pub trigger_bytes: Option<u64>,
    pub pressure_bytes: Option<u64>,
    pub stall_timeout: Option<Duration>,
    pub unknown_observation: bool,
    pub alternating_unknown: bool,
    pub fault: Fault,
    pub entered: Option<crossbeam_channel::Sender<()>>,
    pub release: Option<crossbeam_channel::Receiver<()>>,
}
#[cfg(test)]
static TEST_SETTINGS: Mutex<Vec<(std::path::PathBuf, TestSettings)>> = Mutex::new(Vec::new());
#[cfg(test)]
fn test_settings(root: &Path) -> Option<TestSettings> {
    lock(&TEST_SETTINGS)
        .iter()
        .find(|(key, _)| key == root)
        .map(|(_, value)| value.clone())
}
#[cfg(test)]
pub(super) struct TestGuard(std::path::PathBuf);
#[cfg(test)]
impl TestGuard {
    pub(super) fn install(root: &Path, settings: TestSettings) -> Self {
        lock(&TEST_SETTINGS).push((root.to_path_buf(), settings));
        Self(root.to_path_buf())
    }
}
#[cfg(test)]
impl Drop for TestGuard {
    fn drop(&mut self) {
        lock(&TEST_SETTINGS).retain(|(root, _)| root != &self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, sync::atomic::AtomicUsize};

    struct Fixture {
        _directory: tempfile::TempDir,
        repo: Repository,
        root: PathBuf,
        writer: Connection,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let repo = Repository::new(&directory.path().join("data")).unwrap();
            let root = std::fs::canonicalize(directory.path()).unwrap();
            let writer = repo.connect().unwrap();
            writer
                .execute_batch(
                    "CREATE TABLE checkpoint_fixture(id INTEGER PRIMARY KEY,payload BLOB);",
                )
                .unwrap();
            Self {
                _directory: directory,
                repo,
                root,
                writer,
            }
        }
        fn append(&mut self, count: usize) {
            let tx = self.writer.transaction().unwrap();
            {
                let mut insert = tx
                    .prepare("INSERT INTO checkpoint_fixture(payload) VALUES(zeroblob(4096))")
                    .unwrap();
                for _ in 0..count {
                    insert.execute([]).unwrap();
                }
            }
            tx.commit().unwrap();
        }
    }
    fn low_limits() -> TestSettings {
        TestSettings {
            trigger_bytes: Some(1),
            pressure_bytes: Some(1),
            stall_timeout: Some(Duration::from_millis(180)),
            ..Default::default()
        }
    }

    #[test]
    fn checkpoint_off_does_not_open_or_observe_and_invalid_counts_are_unknown() {
        let fixture = Fixture::new();
        let _hook = TestGuard::install(
            &fixture.root,
            TestSettings {
                fault: Fault::StartSql,
                ..Default::default()
            },
        );
        let mut helper =
            CheckpointHelper::start(&fixture.repo, &fixture.root, Mode::Off, false).unwrap();
        helper.after_commit(&fixture.writer).unwrap();
        helper
            .before_batch(&fixture.writer, &AtomicBool::new(false))
            .unwrap();
        assert!(!helper.shutdown().unwrap().enabled);
        assert!(Observation::from_values(1, 5, 2).is_none());
        assert!(Observation::from_values(0, -1, -1).is_none());
        assert!(Observation::from_values(0, 5, 6).is_none());
        assert_eq!(Observation::from_values(0, 5, 2).unwrap().backlog(), 3);
        assert_eq!(Mode::from_override(None), Mode::Off);
        assert_eq!(Mode::from_override(Some("PASSIVE")), Mode::Off);
        assert_eq!(Mode::from_override(Some("passive")), Mode::Passive);
    }

    #[test]
    fn checkpoint_reader_pin_stalls_without_spinning_and_backfill_allows_restart() {
        let mut fixture = Fixture::new();
        fixture.append(1);
        let reader = fixture.repo.connect().unwrap();
        reader
            .execute_batch("BEGIN; SELECT count(*) FROM checkpoint_fixture;")
            .unwrap();
        fixture.append(48);
        let _hook = TestGuard::install(&fixture.root, low_limits());
        let mut helper =
            CheckpointHelper::start(&fixture.repo, &fixture.root, Mode::Passive, true).unwrap();
        helper.after_commit(&fixture.writer).unwrap();
        let before = Instant::now();
        assert_eq!(
            helper
                .before_batch(&fixture.writer, &AtomicBool::new(false))
                .unwrap_err()
                .code,
            "CHECKPOINT_STALLED"
        );
        assert!(before.elapsed() >= Duration::from_millis(150));
        assert!(before.elapsed() < Duration::from_secs(2));
        let pinned = observe(&fixture.writer).unwrap().unwrap();
        assert!(pinned.backlog() > 0);
        reader.execute_batch("COMMIT").unwrap();
        helper
            .before_batch(&fixture.writer, &AtomicBool::new(false))
            .unwrap();
        let full = observe(&fixture.writer).unwrap().unwrap();
        assert_eq!(full.backlog(), 0);
        assert!(full.log > 0); // PASSIVE backfills; it does not truncate the file.
        fixture.append(1); // A real next transaction can restart the active WAL.
        assert!(observe(&fixture.writer).unwrap().unwrap().log < full.log);
        let stats = helper.shutdown().unwrap();
        assert!(stats.restart_opportunities >= 1);
        assert!(stats.runs <= 8);
    }

    #[test]
    fn checkpoint_large_transaction_coalesces_and_shutdown_joins_inflight_work() {
        let mut fixture = Fixture::new();
        fixture.append(64);
        let (entered, ready) = crossbeam_channel::bounded(1);
        let (release, wait) = crossbeam_channel::bounded(1);
        let mut settings = low_limits();
        settings.entered = Some(entered);
        settings.release = Some(wait);
        let _hook = TestGuard::install(&fixture.root, settings);
        let mut helper =
            CheckpointHelper::start(&fixture.repo, &fixture.root, Mode::Passive, true).unwrap();
        helper.after_commit(&fixture.writer).unwrap();
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 0..20 {
            helper.after_commit(&fixture.writer).unwrap();
        }
        let observation = observe(&fixture.writer).unwrap().unwrap();
        assert!(observation.log * helper.frame_bytes > helper.settings.pressure_bytes);
        let (finished, done) = crossbeam_channel::bounded(1);
        let shared = helper.shared.as_ref().unwrap().clone();
        let shutdown = std::thread::spawn(move || {
            let first = helper.shutdown();
            let second = helper.shutdown();
            finished.send(()).unwrap();
            (first, second)
        });
        let state = lock(&shared.state);
        let (state, _) = shared
            .changed
            .wait_timeout_while(state, Duration::from_secs(2), |state| !state.stopping)
            .unwrap();
        let stopping = state.stopping;
        drop(state);
        assert!(stopping);
        assert!(done.recv_timeout(Duration::from_millis(40)).is_err());
        release.send(()).unwrap();
        drop(release);
        drop(ready);
        let (first, second) = shutdown.join().unwrap();
        let stats = first.unwrap();
        assert_eq!(stats.runs, 1);
        assert!(stats.coalesced_requests >= 19);
        assert_eq!(second.unwrap().runs, stats.runs);
    }

    #[test]
    fn checkpoint_unknown_pressure_is_finite_and_cancel_restores_inline_path() {
        let fixture = Fixture::new();
        let mut settings = low_limits();
        settings.unknown_observation = true;
        let _hook = TestGuard::install(&fixture.root, settings);
        let mut helper =
            CheckpointHelper::start(&fixture.repo, &fixture.root, Mode::Passive, false).unwrap();
        assert_eq!(
            helper
                .before_batch(&fixture.writer, &AtomicBool::new(false))
                .unwrap_err()
                .code,
            "CHECKPOINT_STALLED"
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let notify = cancel.clone();
        let cancellation = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            notify.store(true, Ordering::Release);
        });
        helper.before_batch(&fixture.writer, &cancel).unwrap();
        cancellation.join().unwrap();
        // The same helper no longer observes the unknown hook during accepted-work drain.
        helper.after_commit(&fixture.writer).unwrap();
        helper
            .before_batch(&fixture.writer, &AtomicBool::new(false))
            .unwrap();
        assert!(helper.shutdown().unwrap().unavailable_observations > 0);
        assert_eq!(
            fixture
                .writer
                .query_row("PRAGMA wal_autocheckpoint", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            8000
        );
        assert_eq!(
            fixture
                .writer
                .query_row("PRAGMA synchronous", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn checkpoint_alternating_unknown_and_unchanged_counts_cannot_extend_stall_deadline() {
        let mut fixture = Fixture::new();
        fixture.append(1);
        let reader = fixture.repo.connect().unwrap();
        reader
            .execute_batch("BEGIN; SELECT count(*) FROM checkpoint_fixture;")
            .unwrap();
        fixture.append(16);
        let _hook = TestGuard::install(
            &fixture.root,
            TestSettings {
                alternating_unknown: true,
                ..low_limits()
            },
        );
        let mut helper =
            CheckpointHelper::start(&fixture.repo, &fixture.root, Mode::Passive, false).unwrap();
        let before = Instant::now();
        assert_eq!(
            helper
                .before_batch(&fixture.writer, &AtomicBool::new(false))
                .unwrap_err()
                .code,
            "CHECKPOINT_STALLED"
        );
        assert!(before.elapsed() < Duration::from_secs(1));
        let stats = helper.shutdown().unwrap();
        assert!(stats.unavailable_observations >= 2);
        assert!(stats.observations > stats.unavailable_observations);
        reader.execute_batch("COMMIT").unwrap();
        assert!(!progress(
            None,
            Some(Observation {
                log: 10,
                backfilled: 1
            })
        ));
    }

    #[test]
    fn checkpoint_start_sql_observation_sql_worker_panic_and_close_keep_errors() {
        for (fault, expected) in [
            (Fault::StartSql, "DATABASE_ERROR"),
            (Fault::NoopSql, "DATABASE_ERROR"),
            (Fault::Sql, "DATABASE_ERROR"),
            (Fault::Panic, "CHECKPOINT_WORKER_FAILED"),
            (Fault::Close, "DATABASE_CLOSE"),
        ] {
            let mut fixture = Fixture::new();
            fixture.append(4);
            let (entered, ready) = crossbeam_channel::bounded(1);
            let _hook = TestGuard::install(
                &fixture.root,
                TestSettings {
                    fault,
                    entered: Some(entered),
                    ..low_limits()
                },
            );
            let started =
                CheckpointHelper::start(&fixture.repo, &fixture.root, Mode::Passive, false);
            if fault == Fault::StartSql {
                assert_eq!(started.err().unwrap().code, expected);
            } else {
                let mut helper = started.unwrap();
                if fault == Fault::NoopSql {
                    assert_eq!(
                        helper.after_commit(&fixture.writer).unwrap_err().code,
                        expected
                    );
                    helper.shutdown().unwrap();
                } else if fault == Fault::Close {
                    assert_eq!(helper.shutdown().unwrap_err().code, expected);
                    assert_eq!(helper.shutdown().unwrap_err().code, expected);
                } else {
                    helper.after_commit(&fixture.writer).unwrap();
                    ready.recv_timeout(Duration::from_secs(2)).unwrap();
                    assert_eq!(helper.shutdown().unwrap_err().code, expected);
                }
            }
            fixture
                .writer
                .execute(
                    "INSERT INTO checkpoint_fixture(payload) VALUES(zeroblob(4))",
                    [],
                )
                .unwrap();
            fixture.repo.connect().unwrap().close().unwrap();
        }
    }

    struct BusyBarrier {
        entered: crossbeam_channel::Sender<()>,
        release: crossbeam_channel::Receiver<()>,
        calls: AtomicUsize,
    }
    unsafe extern "C" fn busy_barrier(context: *mut std::ffi::c_void, _: i32) -> i32 {
        let barrier = &*(context as *const BusyBarrier);
        if barrier.calls.fetch_add(1, Ordering::AcqRel) == 0 {
            let _ = barrier.entered.send(());
            let _ = barrier.release.recv_timeout(Duration::from_secs(2));
        }
        0
    }
    #[test]
    fn checkpoint_noop_reads_counts_while_another_connection_holds_checkpoint_lock() {
        let mut fixture = Fixture::new();
        fixture.append(16);
        let observer = fixture.repo.connect().unwrap();
        // FULL takes the checkpoint lock then waits for this own fixture's writer.
        fixture
            .writer
            .execute_batch(
                "BEGIN IMMEDIATE; INSERT INTO checkpoint_fixture(payload) VALUES(zeroblob(4));",
            )
            .unwrap();
        let holder = fixture.repo.connect().unwrap();
        let (entered, ready) = crossbeam_channel::bounded(1);
        let (release, wait) = crossbeam_channel::bounded(1);
        let worker = std::thread::spawn(move || {
            let barrier = Box::new(BusyBarrier {
                entered,
                release: wait,
                calls: AtomicUsize::new(0),
            });
            unsafe {
                rusqlite::ffi::sqlite3_busy_handler(
                    holder.handle(),
                    Some(busy_barrier),
                    (&*barrier as *const BusyBarrier).cast_mut().cast(),
                );
            }
            let result =
                holder.query_row("PRAGMA wal_checkpoint(FULL)", [], |r| r.get::<_, i64>(0));
            unsafe {
                rusqlite::ffi::sqlite3_busy_handler(holder.handle(), None, std::ptr::null_mut());
            }
            holder.close().unwrap();
            result
        });
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        let noop = observe(&observer).unwrap().unwrap();
        assert!(noop.log >= noop.backfilled);
        let busy: i64 = observer
            .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| r.get(0))
            .unwrap();
        assert_eq!(busy, 1); // Proves the FULL holder still owns the checkpoint lock.
        release.send(()).unwrap();
        assert_eq!(worker.join().unwrap().unwrap(), 1);
        fixture.writer.execute_batch("ROLLBACK").unwrap();
    }
}
