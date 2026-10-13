use crate::{model::*, platform};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

// 0/1 retain their persisted meaning. State 2 is an ordinary directory whose
// completeness follows the scan's terminal state without a final bulk rewrite.
pub const NODE_COLUMNS:&str="id,parent_id,path,payload,logical_total,allocated_total,allocated_unknown,file_count,dir_count,children_count,CASE WHEN incomplete=2 THEN (SELECT status NOT IN('completed','completed_with_errors') FROM scans WHERE id=nodes.scan_id) ELSE incomplete END,(stale OR (SELECT history_stale FROM scans WHERE id=nodes.scan_id)),error_code,depth,name,kind,extension,modified,search_path";
struct ViewBudget {
    bytes: usize,
    nodes: usize,
}
#[cfg(test)]
type ViewReadHook = (
    PathBuf,
    crossbeam_channel::Sender<()>,
    crossbeam_channel::Receiver<()>,
);
#[cfg(test)]
static VIEW_READ_HOOK: std::sync::Mutex<Option<ViewReadHook>> = std::sync::Mutex::new(None);
#[cfg(test)]
static VIEW_SERIALIZE_HOOK: std::sync::Mutex<Option<ViewReadHook>> = std::sync::Mutex::new(None);

struct SnapshotReadObservation<'a> {
    scan_id: &'a str,
    started: Option<Instant>,
    started_at_ms: i64,
    completed: bool,
}
impl<'a> SnapshotReadObservation<'a> {
    fn new(scan_id: &'a str) -> Self {
        let enabled = std::env::var_os("SPACEFOLD_PROFILE").is_some();
        Self {
            scan_id,
            started: enabled.then(Instant::now),
            started_at_ms: if enabled {
                chrono::Utc::now().timestamp_millis()
            } else {
                0
            },
            completed: false,
        }
    }
}
impl Drop for SnapshotReadObservation<'_> {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            eprintln!(
                "{}",
                serde_json::json!({"viewReadProfile": {
                    "scanId": self.scan_id, "startedAtMs": self.started_at_ms,
                    "readTransactionMs": started.elapsed().as_secs_f64() * 1000.0,
                    "completed": self.completed
                }})
            );
        }
    }
}

/// Bounded diagnostics on the existing writer; NOOP never checkpoints frames.
#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterObservation {
    /// Upper bounds are 1, 2, 4, 8, 16, 32, 64 ms; the final bucket is >=64 ms.
    commit_buckets: [usize; 8],
    max_commit_ms: f64,
    wal_samples: usize,
    wal_unavailable: usize,
    wal_busy: usize,
    max_wal_frames: i32,
    max_uncheckpointed_frames: i32,
    #[serde(skip)]
    last_sample: Option<Instant>,
}
impl WriterObservation {
    /// Call only when profiling is enabled, after a successful commit.
    pub fn on_commit(&mut self, c: &Connection, scan_id: &str, commit_ms: f64) {
        let bucket = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0]
            .iter()
            .position(|upper| commit_ms < *upper)
            .unwrap_or(7);
        self.commit_buckets[bucket] += 1;
        self.max_commit_ms = self.max_commit_ms.max(commit_ms);
        if self
            .last_sample
            .is_some_and(|last| last.elapsed() < Duration::from_secs(1))
        {
            return;
        }
        self.last_sample = Some(Instant::now());
        self.wal_samples += 1;
        let started = Instant::now();
        let observation = c.query_row("PRAGMA main.wal_checkpoint(NOOP)", [], |r| {
            Ok((
                r.get::<_, i32>(0)?,
                r.get::<_, i32>(1)?,
                r.get::<_, i32>(2)?,
            ))
        });
        let observation_ms = started.elapsed().as_secs_f64() * 1000.0;
        match observation {
            Ok((busy, log_frames, checkpointed_frames))
                if busy == 0 && log_frames >= 0 && checkpointed_frames >= 0 =>
            {
                let remaining = log_frames.saturating_sub(checkpointed_frames);
                self.max_wal_frames = self.max_wal_frames.max(log_frames);
                self.max_uncheckpointed_frames = self.max_uncheckpointed_frames.max(remaining);
                eprintln!(
                    "{}",
                    serde_json::json!({"writerWalProfile": {
                        "scanId": scan_id, "atMs": chrono::Utc::now().timestamp_millis(),
                        "commitMs": commit_ms, "observationMs": observation_ms,
                        "logFrames": log_frames, "checkpointedFrames": checkpointed_frames,
                        "uncheckpointedFrames": remaining
                    }})
                );
            }
            other => {
                self.wal_unavailable += 1;
                let busy = match &other {
                    Ok((busy, _, _)) => *busy != 0,
                    Err(rusqlite::Error::SqliteFailure(error, _)) => {
                        error.code == rusqlite::ErrorCode::DatabaseBusy
                    }
                    _ => false,
                };
                self.wal_busy += usize::from(busy);
                eprintln!(
                    "{}",
                    serde_json::json!({"writerWalProfile": {
                        "scanId": scan_id, "atMs": chrono::Utc::now().timestamp_millis(),
                        "commitMs": commit_ms, "observationMs": observation_ms,
                        "unavailable": true, "busy": busy
                    }})
                );
            }
        }
    }
}

#[cfg(test)]
mod writer_observation_tests {
    use super::*;

    #[test]
    fn checkpoint_noop_reports_frames_without_backfill_or_file_changes() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("checkpoint.sqlite3");
        let mut c = Connection::open(&path).unwrap();
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE items(id INTEGER PRIMARY KEY,data BLOB);").unwrap();
        let tx = c.transaction().unwrap();
        for _ in 0..40 {
            tx.execute("INSERT INTO items(data) VALUES(zeroblob(4096))", [])
                .unwrap();
        }
        tx.commit().unwrap();
        let observe = |mode: &str| {
            c.query_row(&format!("PRAGMA main.wal_checkpoint({mode})"), [], |row| {
                Ok((
                    row.get::<_, i32>(0)?,
                    row.get::<_, i32>(1)?,
                    row.get::<_, i32>(2)?,
                ))
            })
            .unwrap()
        };
        let before = observe("NOOP");
        assert_eq!(before.0, 0);
        assert!(before.1 > 0);
        assert_eq!(before.2, 0);
        let database_before = std::fs::read(&path).unwrap();
        let wal_path = path.with_file_name("checkpoint.sqlite3-wal");
        let wal_before = std::fs::read(&wal_path).unwrap();
        for _ in 0..5 {
            assert_eq!(observe("NOOP"), before);
        }
        assert_eq!(std::fs::read(&path).unwrap(), database_before);
        assert_eq!(std::fs::read(&wal_path).unwrap(), wal_before);
        let checkpoint = observe("PASSIVE");
        assert_eq!(checkpoint.0, 0);
        assert_eq!(checkpoint.1, before.1);
        assert_eq!(checkpoint.2, before.1);
        assert_eq!(observe("NOOP"), checkpoint);
        assert_ne!(std::fs::read(&path).unwrap(), database_before);
    }
}
impl ViewBudget {
    fn charge(&mut self, value: &impl serde::Serialize, nodes: usize) -> Result<()> {
        self.bytes = self.bytes.saturating_add(serde_json::to_vec(value)?.len());
        self.nodes = self.nodes.saturating_add(nodes);
        if self.bytes > 8 * 1024 * 1024 || self.nodes > 10_000 {
            return Err(AppError::new(
                "VIEW_PAYLOAD_BUDGET",
                "视图超出记录或 8 MiB 返回预算，请减少展开目录或所选项",
            ));
        }
        Ok(())
    }
}
/// Version sorting semantics independently of a persisted scan revision.
pub fn page_cursor_key(
    scan_id: &str,
    revision: u32,
    parent: Option<i64>,
    sort: &str,
    filter: &Filter,
) -> Result<String> {
    Ok(serde_json::to_string(&(
        2u8, scan_id, revision, parent, sort, filter,
    ))?)
}

#[cfg(windows)]
fn ordinary_database_filename(canonical_dir: &Path) -> Result<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Component, Prefix};
    let unsupported = || {
        AppError::new("DATABASE_PATH_UNSUPPORTED","扫描数据库需要普通本地数据目录；网络、过长或特殊名称位置不支持，原数据库已保留。扫描来源仍支持长路径和网络目录。")
    };
    let mut components = canonical_dir.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(unsupported());
    };
    let Prefix::VerbatimDisk(drive) = prefix.kind() else {
        return Err(unsupported());
    };
    if !drive.is_ascii_alphabetic() || components.next() != Some(Component::RootDir) {
        return Err(unsupported());
    }
    for component in components {
        let Component::Normal(name) = component else {
            return Err(unsupported());
        };
        let name = name.to_str().ok_or_else(unsupported)?;
        if name.ends_with(['.', ' ']) || name.chars().any(|c| c < ' ' || "<>:\"|?*".contains(c)) {
            return Err(unsupported());
        }
        let stem = name
            .split('.')
            .next()
            .unwrap_or_default()
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        let device = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|tail| {
                matches!(
                    tail,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
        if device {
            return Err(unsupported());
        }
    }
    let root = [u16::from(drive), u16::from(b':'), u16::from(b'\\'), 0];
    // Mapped network drives also cannot host this application's WAL database.
    let kind = unsafe {
        windows::Win32::Storage::FileSystem::GetDriveTypeW(windows::core::PCWSTR(root.as_ptr()))
    };
    if !matches!(kind, 2 | 3 | 6) {
        return Err(unsupported());
    }
    let wide = canonical_dir.as_os_str().encode_wide().collect::<Vec<_>>();
    if !wide.starts_with(&[92, 92, 63, 92]) {
        return Err(unsupported());
    }
    let ordinary =
        PathBuf::from(std::ffi::OsString::from_wide(&wide[4..])).join("spacefold.sqlite3");
    // MAX_PATH includes NUL; reserve the longest SQLite sidecar, '-journal'.
    if ordinary.as_os_str().encode_wide().count() > 251 {
        return Err(unsupported());
    }
    Ok(ordinary)
}

fn verified_sqlite_filename(canonical_dir: &Path, canonical_db: &Path) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let ordinary = ordinary_database_filename(canonical_dir)?;
        let unsupported = || {
            AppError::new(
                "DATABASE_PATH_UNSUPPORTED",
                "数据目录的普通路径不能与原位置无损匹配；原数据库已保留。",
            )
        };
        let source = platform::metadata(canonical_dir)?;
        let candidate = platform::metadata(ordinary.parent().ok_or_else(unsupported)?)?;
        if source.kind != "directory"
            || source.cloud_state.is_some()
            || source.identity.is_none()
            || source.identity != candidate.identity
        {
            return Err(unsupported());
        }
        let existing = match std::fs::symlink_metadata(canonical_db) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if existing {
            let source = platform::metadata(canonical_db)?;
            let candidate = platform::metadata(&ordinary)?;
            if source.kind != "file"
                || source.reparse_tag != 0
                || source.cloud_state.is_some()
                || source.identity.is_none()
                || source.identity != candidate.identity
            {
                return Err(unsupported());
            }
        }
        Ok(ordinary)
    }
    #[cfg(not(windows))]
    {
        let _ = canonical_dir;
        Ok(canonical_db.to_path_buf())
    }
}

#[cfg(all(test, windows))]
mod database_path_tests {
    use super::*;
    use std::os::windows::ffi::OsStringExt;
    #[test]
    fn unc_devices_nonunicode_and_sidecar_budget_are_rejected_before_open() {
        for path in [
            r"\\?\UNC\localhost\share\app-data",
            r"\\?\C:\app\NUL.txt",
            r"\\?\C:\app\CONIN$",
            r"\\?\C:\app\LPT².dat",
            r"\\?\C:\app\trailing.",
            r"\\?\C:\app\trailing ",
            r"\\?\C:\app\bad:name",
        ] {
            assert_eq!(
                ordinary_database_filename(Path::new(path))
                    .unwrap_err()
                    .code,
                "DATABASE_PATH_UNSUPPORTED"
            );
        }
        let mut raw = r"\\?\C:\app\".encode_utf16().collect::<Vec<_>>();
        raw.push(0xd800);
        assert_eq!(
            ordinary_database_filename(&PathBuf::from(std::ffi::OsString::from_wide(&raw)))
                .unwrap_err()
                .code,
            "DATABASE_PATH_UNSUPPORTED"
        );
        let base = r"\\?\C:\";
        let accepted = PathBuf::from(format!("{base}{}", "a".repeat(230)));
        assert_eq!(
            ordinary_database_filename(&accepted)
                .unwrap()
                .as_os_str()
                .to_string_lossy()
                .encode_utf16()
                .count(),
            251
        );
        let rejected = PathBuf::from(format!("{base}{}", "a".repeat(231)));
        assert_eq!(
            ordinary_database_filename(&rejected).unwrap_err().code,
            "DATABASE_PATH_UNSUPPORTED"
        );
    }
}

pub struct Repository {
    pub path: PathBuf,
    pub data_dir: PathBuf,
    // SQLite's Windows VFS treats a leading \\ as UNC, including local \\?\
    // names. Use only an identity-verified ordinary local name for SQLite;
    // retain the canonical paths above for scanning and operation protection.
    sqlite_path: PathBuf,
    id_allocator: std::sync::Mutex<()>,
}
impl Repository {
    pub fn new(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let data_dir = std::fs::canonicalize(data_dir)?;
        let path = data_dir.join("spacefold.sqlite3");
        let sqlite_path = verified_sqlite_filename(&data_dir, &path)?;
        let repo = Self {
            path,
            data_dir,
            sqlite_path,
            id_allocator: std::sync::Mutex::new(()),
        };
        let c = repo.connect()?;
        let has_schema: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_info')",
            [],
            |r| r.get(0),
        )?;
        if has_schema {
            let version: i64 = c.query_row("SELECT version FROM schema_info", [], |r| r.get(0))?;
            if version != 1 {
                return Err(AppError::new(
                    "DATABASE_VERSION",
                    "扫描数据库版本不支持；原数据库已保留",
                ));
            }
        }
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;
    CREATE TABLE IF NOT EXISTS schema_info(version INTEGER NOT NULL);INSERT INTO schema_info SELECT 1 WHERE NOT EXISTS(SELECT 1 FROM schema_info);
    CREATE TABLE IF NOT EXISTS scans(id TEXT PRIMARY KEY,root_path BLOB NOT NULL,source TEXT NOT NULL,started TEXT NOT NULL,finished TEXT,status TEXT NOT NULL,revision INTEGER NOT NULL DEFAULT 1,discovered INTEGER NOT NULL DEFAULT 0,error_count INTEGER NOT NULL DEFAULT 0,disk_json TEXT,notes_json TEXT NOT NULL,stale INTEGER NOT NULL DEFAULT 0,hidden INTEGER NOT NULL DEFAULT 0);
    CREATE TABLE IF NOT EXISTS nodes(scan_id TEXT NOT NULL,id INTEGER NOT NULL,parent_id INTEGER,path BLOB NOT NULL,payload TEXT NOT NULL,name TEXT NOT NULL,kind TEXT NOT NULL,extension TEXT NOT NULL,depth INTEGER NOT NULL,modified TEXT,logical_total INTEGER NOT NULL CHECK(typeof(logical_total)='integer' AND logical_total>=0),allocated_total INTEGER NOT NULL CHECK(typeof(allocated_total)='integer' AND allocated_total>=0),allocated_unknown INTEGER NOT NULL,file_count INTEGER NOT NULL,dir_count INTEGER NOT NULL,children_count INTEGER NOT NULL DEFAULT 0,incomplete INTEGER NOT NULL DEFAULT 1,stale INTEGER NOT NULL DEFAULT 0,error_code TEXT,PRIMARY KEY(scan_id,id),FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS directory_metadata_pending(scan_id TEXT NOT NULL,id INTEGER NOT NULL,PRIMARY KEY(scan_id,id),FOREIGN KEY(scan_id,id) REFERENCES nodes(scan_id,id) ON DELETE CASCADE) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS nodes_parent_name_v2 ON nodes(scan_id,parent_id,name COLLATE NOCASE,id);
    CREATE TABLE IF NOT EXISTS pending_dirs(scan_id TEXT NOT NULL,id INTEGER NOT NULL,path BLOB NOT NULL,depth INTEGER NOT NULL,expected_identity TEXT,metadata_deferred INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(scan_id,id)) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS scan_errors(id INTEGER PRIMARY KEY AUTOINCREMENT,scan_id TEXT NOT NULL,payload TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS operations(id TEXT PRIMARY KEY,scan_id TEXT NOT NULL,kind TEXT NOT NULL,started TEXT NOT NULL,finished TEXT,payload TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS scan_sequence(next_id INTEGER NOT NULL CHECK(typeof(next_id)='integer' AND next_id>0));INSERT INTO scan_sequence SELECT 1 WHERE NOT EXISTS(SELECT 1 FROM scan_sequence);
    UPDATE scans SET status='interrupted',finished=strftime('%Y-%m-%dT%H:%M:%fZ','now'),stale=1 WHERE status IN('running','queued','cancelling');
    DELETE FROM pending_dirs;")?;
        let pending_without_rowid: bool = c.query_row(
            "SELECT wr FROM pragma_table_list WHERE schema='main' AND name='pending_dirs'",
            [],
            |row| row.get(0),
        )?;
        if !pending_without_rowid {
            // Pending work is already discarded when active jobs become interrupted.
            // Rebuild only that empty queue; saved nodes and scans stay in place.
            let tx = c.unchecked_transaction()?;
            tx.execute_batch("DROP TABLE pending_dirs; CREATE TABLE pending_dirs(scan_id TEXT NOT NULL,id INTEGER NOT NULL,path BLOB NOT NULL,depth INTEGER NOT NULL,expected_identity TEXT,metadata_deferred INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(scan_id,id)) WITHOUT ROWID;")?;
            tx.commit()?;
        }
        for (name, definition) in [
            ("expected_identity", "expected_identity TEXT"),
            (
                "metadata_deferred",
                "metadata_deferred INTEGER NOT NULL DEFAULT 0",
            ),
        ] {
            let exists: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('pending_dirs') WHERE name=?1)",
                [name],
                |row| row.get(0),
            )?;
            if !exists {
                c.execute_batch(&format!(
                    "ALTER TABLE pending_dirs ADD COLUMN {definition};"
                ))?;
            }
        }
        let version: i64 = c.query_row("SELECT version FROM schema_info", [], |r| r.get(0))?;
        if version != 1 {
            return Err(AppError::new(
                "DATABASE_VERSION",
                "扫描数据库版本不支持；原数据库已保留",
            ));
        }
        let has_sort_size: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_xinfo('nodes') WHERE name='sort_size')",
            [],
            |r| r.get(0),
        )?;
        if !has_sort_size {
            c.execute_batch("ALTER TABLE nodes ADD COLUMN sort_size INTEGER GENERATED ALWAYS AS (-logical_total) VIRTUAL;")?;
        }
        c.execute_batch("CREATE INDEX IF NOT EXISTS nodes_parent_size_v3 ON nodes(scan_id,parent_id,sort_size,name COLLATE NOCASE,id); CREATE INDEX IF NOT EXISTS nodes_incomplete ON nodes(scan_id,id) WHERE incomplete=1; CREATE INDEX IF NOT EXISTS nodes_errors ON nodes(scan_id,id) WHERE error_code IS NOT NULL; CREATE INDEX IF NOT EXISTS nodes_special_kind_v2 ON nodes(scan_id,kind,parent_id,sort_size,name COLLATE NOCASE,id,logical_total,extension,modified) WHERE kind<>'file'; DROP INDEX IF EXISTS nodes_parent_size_asc_v2; DROP INDEX IF EXISTS nodes_kind; DROP INDEX IF EXISTS nodes_special_kind; DROP INDEX IF EXISTS nodes_parent_size; DROP INDEX IF EXISTS nodes_parent_name; DROP INDEX IF EXISTS nodes_parent_size_v2;")?;
        let has_search_path: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_xinfo('nodes') WHERE name='search_path')",
            [],
            |r| r.get(0),
        )?;
        if !has_search_path {
            c.execute_batch("ALTER TABLE nodes ADD COLUMN search_path TEXT NOT NULL DEFAULT '';UPDATE nodes SET search_path=COALESCE(json_extract(payload,'$.node.displayPath'),'');")?;
        }
        let has_history_stale: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('scans') WHERE name='history_stale')",
            [],
            |row| row.get(0),
        )?;
        if !has_history_stale {
            c.execute_batch(
                "ALTER TABLE scans ADD COLUMN history_stale INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        Ok(repo)
    }
    pub fn connect(&self) -> Result<Connection> {
        let c = Connection::open(&self.sqlite_path)?;
        c.busy_timeout(Duration::from_secs(10))?;
        c.execute_batch("PRAGMA foreign_keys=ON;PRAGMA synchronous=NORMAL;PRAGMA wal_autocheckpoint=8000;PRAGMA cache_size=-16384;PRAGMA temp_store=FILE;")?;
        Ok(c)
    }
    pub fn check_revision(c: &Connection, scan_id: &str, revision: u32) -> Result<()> {
        let actual: Option<u32> = c
            .query_row("SELECT revision FROM scans WHERE id=?1", [scan_id], |r| {
                r.get(0)
            })
            .optional()?;
        match actual {
            Some(v) if v == revision => Ok(()),
            Some(_) => Err(AppError::new(
                "REVISION_CHANGED",
                "扫描结果已经更新，请重新读取",
            )),
            None => Err(AppError::new("SCAN_NOT_FOUND", "未找到扫描记录")),
        }
    }
    pub fn scan(&self, scan_id: &str) -> Result<Scan> {
        Self::scan_conn(&self.connect()?, scan_id)
    }
    pub fn scan_conn(c: &Connection, scan_id: &str) -> Result<Scan> {
        let mut scan=c.query_row("SELECT id,source,started,finished,status,revision,discovered,error_count,disk_json,notes_json,stale FROM scans WHERE id=?1",[scan_id],|r|{let status:String=r.get(4)?;let disk:Option<String>=r.get(8)?;let notes:String=r.get(9)?;Ok(Scan{scan_id:r.get(0)?,root_id:"1".into(),source:r.get(1)?,started_at:r.get(2)?,finished_at:r.get(3)?,incomplete:!matches!(status.as_str(),"completed"),status,revision:r.get(5)?,discovered_count:r.get::<_,i64>(6)?.to_string(),logical_total_bytes:"0".into(),error_count:r.get::<_,i64>(7)?.to_string(),stale:r.get(10)?,disk_space:disk.and_then(|s|serde_json::from_str(&s).ok()),coverage_notes:serde_json::from_str(&notes).unwrap_or_default()})}).optional()?.ok_or_else(||AppError::new("SCAN_NOT_FOUND","未找到扫描记录"))?;
        scan.logical_total_bytes = c
            .query_row(
                "SELECT logical_total FROM nodes WHERE scan_id=?1 AND id=1",
                [scan_id],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
            .to_string();
        Ok(scan)
    }
    pub fn recent(&self) -> Result<Vec<Scan>> {
        let c = self.connect()?;
        let mut stmt =
            c.prepare("SELECT id FROM scans WHERE hidden=0 ORDER BY started DESC LIMIT 12")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.iter().map(|id| Self::scan_conn(&c, id)).collect()
    }
    pub fn insert_scan(&self, id: &str, path: &Path, hidden: bool) -> Result<()> {
        let c = self.connect()?;
        let notes = vec![
            "默认统计文件主数据流的逻辑长度；硬链接按路径重复计数。",
            "未统计备用数据流、目录索引及文件系统元数据。",
            "NTFS 文件的已分配大小与身份尽量在扫描时取得；缺失信息与链接数可在详情或操作前补测，兼容扫描仍可能保留未知值。",
            "目录跳转链接不跟随；云占位仅查询元数据，需要召回的云目录跳过。",
            "扫描不是文件系统原子快照；本应用工作目录已排除。",
            "磁盘 total/free/available 由系统分别返回，包含调用者配额语义。",
        ];
        c.execute("INSERT INTO scans(id,root_path,source,started,status,disk_json,notes_json,hidden) VALUES(?1,?2,?3,?4,'queued',?5,?6,?7)",params![id,platform::encode_path(path),platform::display(path),now(),platform::disk_space(path).map(|d|serde_json::to_string(&d)).transpose()?,serde_json::to_string(&notes)?,hidden])?;
        Ok(())
    }
    pub fn new_scan_id(&self) -> Result<String> {
        // Keep allocation on one writer inside this repository. Thirty-two
        // simultaneous WAL connection writers can otherwise hit SQLITE_PROTOCOL
        // (which SQLite's ordinary busy handler does not retry). The durable
        // transaction still provides uniqueness across processes and restarts.
        let _allocator = self
            .id_allocator
            .lock()
            .map_err(|_| AppError::new("DATABASE_BUSY", "扫描标识分配器不可用，请重试"))?;
        let mut c = self.connect()?;
        let tx = c.transaction()?;
        loop {
            let value: i64 = tx.query_row(
                "UPDATE scan_sequence SET next_id=next_id+1 RETURNING next_id-1",
                [],
                |row| row.get(0),
            )?;
            let id = format!("s{value}");
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM scans WHERE id=?1)",
                [&id],
                |row| row.get(0),
            )?;
            if !exists {
                tx.commit()?;
                return Ok(id);
            }
        }
    }
    pub fn root_path(&self, id: &str) -> Result<PathBuf> {
        let c = self.connect()?;
        Ok(platform::decode_path(c.query_row(
            "SELECT root_path FROM scans WHERE id=?1",
            [id],
            |r| r.get(0),
        )?))
    }
    pub fn node(&self, scan_id: &str, node_id: &str, revision: Option<u32>) -> Result<StoredNode> {
        let c = self.connect()?;
        let tx = c.unchecked_transaction()?;
        if let Some(r) = revision {
            Self::check_revision(&tx, scan_id, r)?
        }
        Self::node_conn(&tx, scan_id, parse_id(node_id)?)
    }
    pub fn node_conn(c: &Connection, scan_id: &str, node_id: i64) -> Result<StoredNode> {
        let sql = format!("SELECT {NODE_COLUMNS} FROM nodes WHERE scan_id=?1 AND id=?2");
        c.query_row(&sql, params![scan_id, node_id], read_node)
            .optional()?
            .ok_or_else(|| AppError::new("NODE_NOT_FOUND", "项目已不存在，请刷新目录"))
    }
    pub fn ancestors_conn(c: &Connection, scan_id: &str, id: i64) -> Result<Vec<StoredNode>> {
        let mut result = vec![];
        let mut next = Self::node_conn(c, scan_id, id)?.node.parent_id;
        let mut seen = HashSet::new();
        while let Some(parent) = next {
            let p = parse_id(&parent)?;
            if !seen.insert(p) {
                return Err(AppError::new("CORRUPT_HIERARCHY", "扫描层级记录损坏"));
            }
            let n = Self::node_conn(c, scan_id, p)?;
            next = n.node.parent_id.clone();
            result.push(n);
            if result.len() > 4096 {
                return Err(AppError::new("DEPTH_LIMIT", "目录深度超过支持上限"));
            }
        }
        result.reverse();
        Ok(result)
    }
    pub fn ancestors(&self, scan_id: &str, id: &str, revision: u32) -> Result<Vec<Node>> {
        let c = self.connect()?;
        let tx = c.unchecked_transaction()?;
        Self::check_revision(&tx, scan_id, revision)?;
        Ok(Self::ancestors_conn(&tx, scan_id, parse_id(id)?)?
            .into_iter()
            .map(|n| n.node)
            .collect())
    }
    pub fn normalize_conn(
        c: &Connection,
        scan_id: &str,
        ids: &[String],
    ) -> Result<(Vec<StoredNode>, Vec<String>)> {
        if ids.len() > 4096 {
            return Err(AppError::new("SELECTION_LIMIT", "单次选择最多 4096 项"));
        }
        let set: HashSet<i64> = ids.iter().map(|s| parse_id(s)).collect::<Result<_>>()?;
        let mut roots = vec![];
        let mut covered = vec![];
        let mut ordered = set.iter().copied().collect::<Vec<_>>();
        ordered.sort_unstable();
        for id in ordered {
            let node = Self::node_conn(c, scan_id, id)?;
            let anc = Self::ancestors_conn(c, scan_id, id)?;
            if anc
                .iter()
                .any(|n| set.contains(&parse_id(&n.node.node_id).unwrap_or(0)))
            {
                covered.push(id.to_string())
            } else {
                roots.push(node)
            }
        }
        Ok((roots, covered))
    }
    pub fn selection(
        &self,
        scan_id: &str,
        ids: &[String],
        revision: u32,
    ) -> Result<SelectionSummary> {
        let c = self.connect()?;
        let tx = c.unchecked_transaction()?;
        Self::check_revision(&tx, scan_id, revision)?;
        Self::selection_conn(&tx, scan_id, ids)
    }
    fn selection_conn(c: &Connection, scan_id: &str, ids: &[String]) -> Result<SelectionSummary> {
        let (nodes, covered) = Self::normalize_conn(c, scan_id, ids)?;
        Self::selection_nodes(nodes.into_iter().map(|node| node.node).collect(), covered)
    }
    fn selection_nodes(nodes: Vec<Node>, covered: Vec<String>) -> Result<SelectionSummary> {
        let mut bytes = 0i64;
        let mut alloc = 0i64;
        let mut unknown = 0i64;
        for n in &nodes {
            bytes = checked_add(bytes, parse_bytes(&n.logical_total_bytes)?)?;
            alloc = checked_add(alloc, parse_bytes(&n.allocated_known_total_bytes)?)?;
            unknown = checked_add(unknown, parse_bytes(&n.allocated_unknown_count)?)?
        }
        let mutable = !nodes.is_empty() && nodes.iter().all(|n| n.parent_id.is_some());
        let single = nodes.len() == 1;
        Ok(SelectionSummary {
            normalized_ids: nodes.iter().map(|n| n.node_id.clone()).collect(),
            covered_ids: covered,
            count: nodes.len().to_string(),
            logical_total_bytes: bytes.to_string(),
            allocated_known_total_bytes: alloc.to_string(),
            allocated_unknown_count: unknown.to_string(),
            can_open: single,
            can_rename: single && mutable,
            can_move: mutable,
            can_trash: mutable,
            nodes,
        })
    }
    fn visit_ancestors_conn(
        c: &Connection,
        scan_id: &str,
        mut parent: Option<String>,
        mut visit: impl FnMut(Node) -> Result<()>,
    ) -> Result<()> {
        let mut seen = HashSet::new();
        while let Some(id) = parent {
            let id = parse_id(&id)?;
            if !seen.insert(id) {
                return Err(AppError::new("CORRUPT_HIERARCHY", "扫描层级记录损坏"));
            }
            if seen.len() > 4096 {
                return Err(AppError::new("DEPTH_LIMIT", "目录深度超过支持上限"));
            }
            let node = Self::node_conn(c, scan_id, id)?.node;
            parent = node.parent_id.clone();
            visit(node)?;
        }
        Ok(())
    }
    pub fn page(&self, r: &PageRequest, search: bool) -> Result<NodePage> {
        let c = self.connect()?;
        let tx = c.unchecked_transaction()?;
        Self::page_conn(&tx, r, search)
    }
    fn page_conn(tx: &Connection, r: &PageRequest, search: bool) -> Result<NodePage> {
        use rusqlite::types::Value;
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Cursor {
            key: String,
            id: i64,
            name: String,
            size: i64,
            modified: String,
        }
        Self::check_revision(tx, &r.scan_id, r.revision)?;
        let limit = r.page_size.unwrap_or(200).clamp(1, 500);
        let parent = if search {
            None
        } else {
            Some(parse_id(r.parent_id.as_deref().unwrap_or("1"))?)
        };
        let f = r.filter.clone().unwrap_or_default();
        let keyword = f.keyword.clone().unwrap_or_default();
        if keyword.len() > 2048 {
            return Err(AppError::new("QUERY_LIMIT", "查询关键词过长"));
        }
        let kind = f.kind.clone().unwrap_or_else(|| "all".into());
        let ext = f
            .extension
            .clone()
            .unwrap_or_default()
            .trim_start_matches('.')
            .to_lowercase();
        let min = f
            .min_bytes
            .as_deref()
            .map(parse_bytes)
            .transpose()?
            .unwrap_or(0);
        let sort = r.sort.as_deref().unwrap_or("size_desc");
        let ordering = match sort {
            "size_desc" => "sort_size ASC,name COLLATE NOCASE ASC,id ASC",
            "size_asc" => "sort_size DESC,name COLLATE NOCASE DESC,id DESC",
            "name_asc" => "name COLLATE NOCASE ASC,id ASC",
            "name_desc" => "name COLLATE NOCASE DESC,id DESC",
            "modified_desc" => "COALESCE(modified,'') DESC,name COLLATE NOCASE,id ASC",
            _ => return Err(AppError::new("INVALID_SORT", "排序方式无效")),
        };
        let key = page_cursor_key(&r.scan_id, r.revision, parent, sort, &f)?;
        let cursor = r
            .cursor
            .as_deref()
            .filter(|s| !s.is_empty() && *s != "0")
            .map(|s| {
                serde_json::from_str::<Cursor>(s)
                    .map_err(|_| AppError::new("INVALID_CURSOR", "分页游标无效"))
            })
            .transpose()?;
        if cursor
            .as_ref()
            .is_some_and(|c| c.key != key || c.size < 0 || c.id <= 0)
        {
            return Err(AppError::new("INVALID_CURSOR", "分页游标与查询范围不符"));
        }
        let mut args = vec![Value::Text(r.scan_id.clone())];
        let mut clause = String::from("scan_id=?1");
        if let Some(parent) = parent {
            args.push(Value::Integer(parent));
            clause.push_str(" AND parent_id=?2");
        } else {
            clause.push_str(" AND id<>1");
        }
        let mut bind = |v: Value| {
            args.push(v);
            format!("?{}", args.len())
        };
        if !keyword.is_empty() {
            let pattern = format!(
                "%{}%",
                keyword
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            );
            let q = bind(Value::Text(pattern));
            clause.push_str(&format!(
                " AND (name LIKE {q} ESCAPE '\\' OR search_path LIKE {q} ESCAPE '\\')"
            ));
        }
        if kind != "all" {
            let q = bind(Value::Text(kind.clone()));
            clause.push_str(&format!(" AND kind={q}"));
            if kind != "file" {
                clause.push_str(" AND kind<>'file'");
            }
        }
        if !ext.is_empty() {
            let q = bind(Value::Text(ext));
            clause.push_str(&format!(" AND extension={q}"));
        }
        if min > 0 {
            let q = bind(Value::Integer(min));
            clause.push_str(&format!(" AND logical_total>={q}"));
        }
        drop(bind);
        let source = if kind != "all" && kind != "file" {
            "nodes INDEXED BY nodes_special_kind_v2"
        } else {
            "nodes"
        };
        let simple =
            keyword.is_empty() && f.extension.as_deref().unwrap_or("").is_empty() && min == 0;
        let cached_total = if simple && kind == "all" {
            if let Some(parent) = parent {
                Some(tx.query_row(
                    "SELECT children_count FROM nodes WHERE scan_id=?1 AND id=?2",
                    params![r.scan_id, parent],
                    |row| row.get(0),
                )?)
            } else {
                Some(tx.query_row(
                    "SELECT discovered-1 FROM scans WHERE id=?1",
                    [&r.scan_id],
                    |row| row.get(0),
                )?)
            }
        } else if simple && matches!(kind.as_str(), "file" | "directory") {
            let (files, dirs, parent_kind): (i64, i64, String) = tx.query_row(
                "SELECT file_count,dir_count,kind FROM nodes WHERE scan_id=?1 AND id=?2",
                params![r.scan_id, parent.unwrap_or(1)],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            if parent.is_none() {
                Some(if kind == "file" { files } else { dirs })
            } else if parent_kind != "directory" {
                Some(0)
            } else if dirs == 0 {
                Some(if kind == "file" { files } else { 0 })
            } else {
                None
            }
        } else {
            None
        };
        let total: i64 = if let Some(total) = cached_total {
            total
        } else {
            tx.query_row(
                &format!("SELECT COUNT(*) FROM {source} WHERE {clause}"),
                rusqlite::params_from_iter(&args),
                |row| row.get(0),
            )?
        };
        let mut ranges = Vec::new();
        if let Some(cursor) = cursor {
            let mut bind = |v: Value| {
                args.push(v);
                format!("?{}", args.len())
            };
            let id = bind(Value::Integer(cursor.id));
            let name = bind(Value::Text(cursor.name));
            let tail=format!("(name COLLATE NOCASE>{name} COLLATE NOCASE OR (name COLLATE NOCASE={name} COLLATE NOCASE AND id>{id}))");
            match sort {
                "size_desc" | "size_asc" => {
                    let size = bind(Value::Integer(-cursor.size));
                    let operator = if sort == "size_desc" { ">" } else { "<" };
                    ranges.extend([
                        format!("sort_size{operator}{size}"),
                        format!("sort_size={size} AND name COLLATE NOCASE{operator}{name} COLLATE NOCASE"),
                        format!("sort_size={size} AND name COLLATE NOCASE={name} COLLATE NOCASE AND id{operator}{id}"),
                    ]);
                }
                "name_asc" | "name_desc" => {
                    let operator = if sort == "name_asc" { ">" } else { "<" };
                    ranges.extend([
                        format!("name COLLATE NOCASE{operator}{name} COLLATE NOCASE"),
                        format!("name COLLATE NOCASE={name} COLLATE NOCASE AND id{operator}{id}"),
                    ]);
                }
                "modified_desc" => {
                    let modified = bind(Value::Text(cursor.modified));
                    ranges.push(format!("(COALESCE(modified,'')<{modified} OR (COALESCE(modified,'')={modified} AND {tail}))"));
                }
                _ => unreachable!(),
            }
        }
        args.push(Value::Integer(limit as i64 + 1));
        let limit_parameter = format!("?{}", args.len());
        let sql = if ranges.is_empty() && source == "nodes" {
            format!("SELECT {NODE_COLUMNS} FROM nodes WHERE {clause} ORDER BY {ordering} LIMIT {limit_parameter}")
        } else {
            // SQLite cannot fully seek a tuple containing COLLATE NOCASE. Disjoint
            // scalar ranges let every branch seek the complete index prefix, even
            // when a million siblings have identical sizes. Only bounded keys are
            // merged; payloads and raw paths are fetched after selecting the page.
            if ranges.is_empty() {
                ranges.push("1".into());
            }
            let branches = ranges.into_iter().map(|range| format!(
                "SELECT * FROM (SELECT id,sort_size,logical_total,name,modified FROM {source} WHERE {clause} AND {range} ORDER BY {ordering} LIMIT {limit_parameter})"
            )).collect::<Vec<_>>().join(" UNION ALL ");
            format!("SELECT {NODE_COLUMNS} FROM (SELECT id AS result_id FROM ({branches}) ORDER BY {ordering} LIMIT {limit_parameter}) page_keys CROSS JOIN nodes ON nodes.scan_id=?1 AND nodes.id=page_keys.result_id ORDER BY {ordering}")
        };
        let mut stmt = tx.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(&args))?;
        let mut nodes = vec![];
        let mut payload_bytes = 0usize;
        let mut more = false;
        while let Some(row) = rows.next()? {
            let node = read_node(row)?.node;
            let bytes = serde_json::to_vec(&node)?.len();
            if !nodes.is_empty()
                && (nodes.len() >= limit as usize || payload_bytes + bytes > 2 * 1024 * 1024)
            {
                more = true;
                break;
            }
            payload_bytes += bytes;
            nodes.push(node);
        }
        let next_cursor = if more {
            nodes
                .last()
                .map(|n| {
                    serde_json::to_string(&Cursor {
                        key,
                        id: parse_id(&n.node_id).unwrap(),
                        name: n.name.clone(),
                        size: parse_bytes(&n.logical_total_bytes).unwrap(),
                        modified: n.modified_at.clone().unwrap_or_default(),
                    })
                })
                .transpose()?
        } else {
            None
        };
        Ok(NodePage {
            revision: r.revision,
            nodes,
            next_cursor,
            total_count: total.to_string(),
            page_cursor: r.cursor.clone(),
        })
    }
    pub fn node_page(
        &self,
        scan_id: &str,
        node_id: &str,
        revision: u32,
        sort: &str,
        page_size: u32,
    ) -> Result<NodePage> {
        let c = self.connect()?;
        let tx = c.unchecked_transaction()?;
        Self::node_page_conn(&tx, scan_id, node_id, revision, sort, page_size)
    }
    fn node_page_conn(
        tx: &Connection,
        scan_id: &str,
        node_id: &str,
        revision: u32,
        sort: &str,
        page_size: u32,
    ) -> Result<NodePage> {
        Self::check_revision(tx, scan_id, revision)?;
        let target = Self::node_conn(tx, scan_id, parse_id(node_id)?)?;
        let Some(parent) = target.node.parent_id.as_deref() else {
            return Ok(NodePage {
                revision,
                nodes: vec![target.node],
                next_cursor: None,
                total_count: "1".into(),
                page_cursor: None,
            });
        };
        let parent_id = parse_id(parent)?;
        let (fields, operator, order, value) = match sort {
            "size_desc" => (
                "sort_size,name COLLATE NOCASE,id",
                "<",
                "sort_size DESC,name COLLATE NOCASE DESC,id DESC",
                -parse_bytes(&target.node.logical_total_bytes)?,
            ),
            "size_asc" => (
                "sort_size,name COLLATE NOCASE,id",
                ">",
                "sort_size,name COLLATE NOCASE,id",
                -parse_bytes(&target.node.logical_total_bytes)?,
            ),
            "name_asc" => (
                "name COLLATE NOCASE,id",
                "<",
                "name COLLATE NOCASE DESC,id DESC",
                0,
            ),
            "name_desc" => ("name COLLATE NOCASE,id", ">", "name COLLATE NOCASE,id", 0),
            _ => return Err(AppError::new("INVALID_SORT", "定位分页排序方式无效")),
        };
        let full = sort.starts_with("size_");
        let sql=format!("SELECT {NODE_COLUMNS} FROM nodes WHERE scan_id=?1 AND parent_id=?2 AND ({fields}){operator}({}) ORDER BY {order} LIMIT 1",if full{"?3,?4,?5"}else{"?3,?4"});
        let mut stmt = tx.prepare(&sql)?;
        let previous = if full {
            stmt.query_row(
                params![
                    scan_id,
                    parent_id,
                    value,
                    target.node.name,
                    parse_id(node_id)?
                ],
                read_node,
            )
            .optional()?
        } else {
            stmt.query_row(
                params![scan_id, parent_id, target.node.name, parse_id(node_id)?],
                read_node,
            )
            .optional()?
        };
        let cursor=previous.map(|previous|{let key=page_cursor_key(scan_id,revision,Some(parent_id),sort,&Filter::default())?;Ok::<_,AppError>(serde_json::json!({"key":key,"id":parse_id(&previous.node.node_id)?,"name":previous.node.name,"size":parse_bytes(&previous.node.logical_total_bytes)?,"modified":previous.node.modified_at.unwrap_or_default()}).to_string())}).transpose()?;
        Self::page_conn(
            tx,
            &PageRequest {
                scan_id: scan_id.into(),
                parent_id: Some(parent.into()),
                revision,
                sort: Some(sort.into()),
                filter: None,
                cursor,
                page_size: Some(page_size),
            },
            false,
        )
    }
    pub fn chart(
        &self,
        scan_id: &str,
        scope_id: &str,
        revision: u32,
        depth: u32,
        budget: u32,
    ) -> Result<ChartData> {
        let c = self.connect()?;
        let tx = c.unchecked_transaction()?;
        Self::chart_conn(&tx, scan_id, scope_id, revision, depth, budget)
    }
    fn chart_conn(
        tx: &Connection,
        scan_id: &str,
        scope_id: &str,
        revision: u32,
        depth: u32,
        budget: u32,
    ) -> Result<ChartData> {
        Self::check_revision(tx, scan_id, revision)?;
        let mut left = budget.clamp(2, 2000);
        let mut truncated = false;
        let root = chart_node(
            tx,
            scan_id,
            parse_id(scope_id)?,
            depth.clamp(1, 4),
            &mut left,
            &mut truncated,
            None,
        )?;
        Ok(ChartData {
            revision,
            root,
            truncated,
        })
    }
    /// Build one committed UI view without holding the scanner's writer mutex.
    /// Every helper below reads this transaction, never a second connection.
    pub fn view_snapshot(&self, request: &ViewSnapshotRequest) -> Result<ViewSnapshot> {
        const PAYLOAD_LIMIT: usize = 8 * 1024 * 1024;
        const NODE_LIMIT: usize = 10_000;
        if request.expanded_ids.len() > 40 || request.page_anchors.len() > 40 {
            return Err(AppError::new(
                "VIEW_LIMIT",
                "单次视图最多包含 40 个展开目录和分页锚点",
            ));
        }
        if request.selected_ids.len() > 4096 {
            return Err(AppError::new("SELECTION_LIMIT", "单次选择最多 4096 项"));
        }
        let sort = request.sort.as_deref().unwrap_or("size_desc");
        if !matches!(sort, "size_desc" | "size_asc" | "name_asc" | "name_desc") {
            return Err(AppError::new("INVALID_SORT", "视图排序方式无效"));
        }
        let c = self.connect()?;
        let mut read_observation = SnapshotReadObservation::new(&request.scan_id);
        let tx = c.unchecked_transaction()?;
        // This first SELECT establishes the WAL read snapshot. Future writer
        // commits cannot advance its revision or any rows read by this request.
        let scan = Self::scan_conn(&tx, &request.scan_id)?;
        #[cfg(test)]
        {
            let hook = VIEW_READ_HOOK
                .lock()
                .unwrap()
                .as_ref()
                .filter(|hook| hook.0 == self.path)
                .cloned();
            if let Some((_, entered, release)) = hook {
                entered.send(()).unwrap();
                release.recv().unwrap();
            }
        }
        let revision = scan.revision;
        let mut budget = ViewBudget {
            bytes: 8192,
            nodes: 0,
        };
        let mut nodes = std::collections::BTreeMap::<String, Node>::new();
        let mut expanded = std::collections::BTreeSet::<String>::new();
        let mut add_node = |node: Node, expand: bool| -> Result<()> {
            if expand && node.kind == "directory" {
                expanded.insert(node.node_id.clone());
            }
            if !nodes.contains_key(&node.node_id) {
                budget.charge(&node, 1)?;
                nodes.insert(node.node_id.clone(), node);
            }
            Ok(())
        };
        let root = Self::node_conn(&tx, &request.scan_id, parse_id(&scan.root_id)?)?.node;
        add_node(root.clone(), false)?;
        let scope = match Self::node_conn(&tx, &request.scan_id, parse_id(&request.scope_id)?) {
            Ok(node) if node.node.kind == "directory" => node.node,
            Ok(_) => root,
            Err(error) if error.code == "NODE_NOT_FOUND" => root,
            Err(error) => return Err(error),
        };
        let scope_id = scope.node_id.clone();
        Self::visit_ancestors_conn(&tx, &request.scan_id, scope.parent_id.clone(), |node| {
            add_node(node, false)
        })?;
        add_node(scope, false)?;
        for id in &request.expanded_ids {
            match Self::node_conn(&tx, &request.scan_id, parse_id(id)?) {
                Ok(node) => add_node(node.node, true)?,
                Err(error) if error.code == "NODE_NOT_FOUND" => {}
                Err(error) => return Err(error),
            }
        }
        let mut selected_ids = Vec::new();
        let mut selected_set = HashSet::new();
        for id in &request.selected_ids {
            let parsed = parse_id(id)?;
            if !selected_set.insert(parsed) {
                continue;
            }
            match Self::node_conn(&tx, &request.scan_id, parsed) {
                Ok(node) => {
                    selected_ids.push(node.node.node_id.clone());
                    Self::visit_ancestors_conn(
                        &tx,
                        &request.scan_id,
                        node.node.parent_id.clone(),
                        |ancestor| add_node(ancestor, false),
                    )?;
                    add_node(node.node, false)?;
                }
                Err(error) if error.code == "NODE_NOT_FOUND" => {}
                Err(error) => return Err(error),
            }
        }
        let primary_id = request
            .primary_id
            .as_ref()
            .filter(|id| selected_ids.contains(id))
            .cloned()
            .or_else(|| selected_ids.first().cloned());
        if scope_id != request.scope_id {
            // Only a vanished/changed scope resets expansion. A normal refresh
            // must not reopen a directory the user deliberately collapsed.
            expanded.clear();
            expanded.insert(scan.root_id.clone());
        }
        let alive = selected_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let mut ordered_selection = selected_ids
            .iter()
            .map(|id| Ok((parse_id(id)?, id)))
            .collect::<Result<Vec<_>>>()?;
        ordered_selection.sort_unstable_by_key(|entry| entry.0);
        let mut normalized = Vec::new();
        let mut covered = Vec::new();
        for (_, id) in ordered_selection {
            let node = nodes.get(id).unwrap();
            let mut parent = node.parent_id.as_deref();
            let mut is_covered = false;
            while let Some(parent_id) = parent {
                if alive.contains(parent_id) {
                    is_covered = true;
                    break;
                }
                parent = nodes
                    .get(parent_id)
                    .ok_or_else(|| AppError::new("CORRUPT_HIERARCHY", "扫描层级记录损坏"))?
                    .parent_id
                    .as_deref();
            }
            if is_covered {
                covered.push(id.clone());
            } else {
                normalized.push(node.clone());
            }
        }
        let summary = Self::selection_nodes(normalized, covered)?;
        budget.charge(&summary, summary.nodes.len())?;
        let mut chart_left = 400;
        let mut chart_truncated = false;
        let chart = ChartData {
            revision,
            root: chart_node(
                &tx,
                &request.scan_id,
                parse_id(&scope_id)?,
                2,
                &mut chart_left,
                &mut chart_truncated,
                Some(&mut budget),
            )?,
            truncated: chart_truncated,
        };
        let mut pages = Vec::new();
        let mut page_parents = expanded.clone();
        page_parents.insert(scan.root_id.clone());
        page_parents.insert(scope_id.clone());
        for parent_id in &page_parents {
            let anchor = if let Some(anchor) = request
                .page_anchors
                .iter()
                .find(|anchor| &anchor.parent_id == parent_id)
            {
                match Self::node_conn(&tx, &request.scan_id, parse_id(&anchor.first_id)?) {
                    Ok(node) if node.node.parent_id.as_ref() == Some(parent_id) => Some(node),
                    Ok(_) => None,
                    Err(error) if error.code == "NODE_NOT_FOUND" => None,
                    Err(error) => return Err(error),
                }
            } else {
                None
            };
            let page = if let Some(anchor) = anchor {
                Self::node_page_conn(
                    &tx,
                    &request.scan_id,
                    &anchor.node.node_id,
                    revision,
                    sort,
                    200,
                )?
            } else {
                Self::page_conn(
                    &tx,
                    &PageRequest {
                        scan_id: request.scan_id.clone(),
                        parent_id: Some(parent_id.clone()),
                        revision,
                        sort: Some(sort.into()),
                        filter: None,
                        cursor: None,
                        page_size: Some(200),
                    },
                    false,
                )?
            };
            budget.charge(&page, page.nodes.len())?;
            pages.push(ViewPage {
                parent_id: parent_id.clone(),
                page,
            });
        }
        let mut details = Vec::new();
        if let Some(primary_id) = &primary_id {
            if nodes
                .get(primary_id)
                .is_some_and(|node| node.kind == "directory")
            {
                let page = Self::page_conn(
                    &tx,
                    &PageRequest {
                        scan_id: request.scan_id.clone(),
                        parent_id: Some(primary_id.clone()),
                        revision,
                        sort: Some("size_desc".into()),
                        filter: None,
                        cursor: None,
                        page_size: Some(5),
                    },
                    false,
                )?;
                budget.charge(&page.nodes, page.nodes.len())?;
                details.push(ViewDetails {
                    node_id: primary_id.clone(),
                    nodes: page.nodes,
                });
            }
        }
        let snapshot = ViewSnapshot {
            scan,
            scope_id,
            primary_id,
            expanded_ids: expanded.into_iter().collect(),
            selected_ids,
            nodes: nodes.into_values().collect(),
            pages,
            chart,
            summary,
            details,
        };
        // Every cohort field now owns its data. Release the WAL read snapshot
        // before exact serialization; it no longer needs any database queries.
        tx.commit()?;
        read_observation.completed = true;
        drop(read_observation);
        #[cfg(test)]
        {
            let hook = VIEW_SERIALIZE_HOOK
                .lock()
                .unwrap()
                .as_ref()
                .filter(|hook| hook.0 == self.path)
                .cloned();
            if let Some((_, entered, release)) = hook {
                entered.send(()).unwrap();
                release.recv().unwrap();
            }
        }
        // Keep both output guards exact, including envelopes/string arrays. The
        // incremental guard above also bounds construction before serialization.
        if budget.nodes > NODE_LIMIT || serde_json::to_vec(&snapshot)?.len() > PAYLOAD_LIMIT {
            return Err(AppError::new(
                "VIEW_PAYLOAD_BUDGET",
                "视图超出记录或 8 MiB 返回预算，请减少展开目录或所选项",
            ));
        }
        Ok(snapshot)
    }
    pub fn errors(&self, scan_id: &str, cursor: Option<&str>, page_size: u32) -> Result<ErrorPage> {
        let c = self.connect()?;
        let from = cursor
            .unwrap_or("0")
            .parse::<i64>()
            .map_err(|_| AppError::new("INVALID_CURSOR", "分页游标无效"))?;
        let limit = page_size.clamp(1, 200);
        let mut stmt = c.prepare(
            "SELECT id,payload FROM scan_errors WHERE scan_id=?1 AND id>?2 ORDER BY id LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![scan_id, from, limit + 1], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let more = rows.len() > limit as usize;
        let selected = &rows[..rows.len().min(limit as usize)];
        let errors = selected
            .iter()
            .map(|(_, s)| serde_json::from_str(s).map_err(AppError::from))
            .collect::<Result<Vec<_>>>()?;
        Ok(ErrorPage {
            errors,
            next_cursor: if more {
                selected.last().map(|(id, _)| id.to_string())
            } else {
                None
            },
        })
    }
    pub fn error_conn(c: &Connection, scan_id: &str, error: ScanError) -> Result<()> {
        c.execute(
            "INSERT INTO scan_errors(scan_id,payload) VALUES(?1,?2)",
            params![scan_id, serde_json::to_string(&error)?],
        )?;
        c.execute(
            "UPDATE scans SET error_count=error_count+1 WHERE id=?1",
            [scan_id],
        )?;
        if let Some(id) = error.node_id {
            c.execute(
                "UPDATE nodes SET incomplete=1,error_code=?3 WHERE scan_id=?1 AND id=?2",
                params![scan_id, parse_id(&id)?, error.error.code],
            )?;
        }
        Ok(())
    }
    pub fn finish(&self, scan_id: &str, status: &str) -> Result<Scan> {
        let started = Instant::now();
        let connection = self.connect()?;
        let c = connection.unchecked_transaction()?;
        let connect_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        c.execute(
            "UPDATE scans SET status=?2,finished=?3,revision=revision+1 WHERE id=?1",
            params![scan_id, status, now()],
        )?;
        let status_ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut clear_incomplete_ms = 0.0;
        let mut error_ancestors_ms = 0.0;
        if matches!(status, "completed" | "completed_with_errors") {
            let started = Instant::now();
            c.execute("UPDATE nodes INDEXED BY nodes_incomplete SET incomplete=CASE WHEN error_code IS NULL THEN 0 ELSE 1 END WHERE scan_id=?1 AND incomplete=1",[scan_id])?;
            clear_incomplete_ms = started.elapsed().as_secs_f64() * 1000.0;
            if status == "completed_with_errors" {
                let started = Instant::now();
                c.execute("WITH RECURSIVE affected(id) AS (SELECT id FROM nodes INDEXED BY nodes_errors WHERE scan_id=?1 AND error_code IS NOT NULL UNION SELECT n.parent_id FROM affected a CROSS JOIN nodes n ON n.scan_id=?1 AND n.id=a.id WHERE n.parent_id IS NOT NULL) UPDATE nodes SET incomplete=1 WHERE scan_id=?1 AND id IN affected",[scan_id])?;
                error_ancestors_ms = started.elapsed().as_secs_f64() * 1000.0;
            }
        }
        let started = Instant::now();
        c.execute("DELETE FROM pending_dirs WHERE scan_id=?1", [scan_id])?;
        let delete_queue_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        let result = Self::scan_conn(&c, scan_id)?;
        let scan_read_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        c.commit()?;
        let commit_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        drop(connection);
        let close_ms = started.elapsed().as_secs_f64() * 1000.0;
        if std::env::var_os("SPACEFOLD_PROFILE").is_some() {
            eprintln!(
                "{}",
                serde_json::json!({"finishProfile": {
                    "scanId": scan_id, "status": status, "connectMs": connect_ms,
                    "statusMs": status_ms, "clearIncompleteMs": clear_incomplete_ms,
                    "errorAncestorsMs": error_ancestors_ms, "deleteQueueMs": delete_queue_ms,
                    "scanReadMs": scan_read_ms, "commitMs": commit_ms, "closeMs": close_ms
                }})
            );
        }
        Ok(result)
    }
    pub fn mark_stale(&self, scan_id: &str) -> Result<u32> {
        let c = self.connect()?;
        let tx = c.unchecked_transaction()?;
        tx.execute("UPDATE nodes SET stale=1 WHERE scan_id=?1", [scan_id])?;
        tx.execute(
            "UPDATE scans SET stale=1,revision=revision+1 WHERE id=?1",
            [scan_id],
        )?;
        let revision = Self::scan_conn(&tx, scan_id)?.revision;
        tx.commit()?;
        Ok(revision)
    }
    pub fn load_history(&self, scan_id: &str) -> Result<Scan> {
        let connection = self.connect()?;
        let tx = connection.unchecked_transaction()?;
        let scan = Self::scan_conn(&tx, scan_id)?;
        if matches!(scan.status.as_str(), "queued" | "running" | "cancelling") {
            return Err(AppError::new("SCAN_BUSY", "活动扫描不能作为历史快照加载"));
        }
        // Inherit uncertainty without rewriting a million node rows. Only a
        // complete root rescan can clear this global history flag.
        tx.execute("UPDATE scans SET history_stale=1,stale=1,revision=revision+1 WHERE id=?1 AND history_stale=0",[scan_id])?;
        let loaded = Self::scan_conn(&tx, scan_id)?;
        tx.commit()?;
        Ok(loaded)
    }
    pub fn log_operation(
        &self,
        id: &str,
        scan_id: &str,
        kind: &str,
        results: &[OperationResult],
    ) -> Result<()> {
        let c = self.connect()?;
        c.execute("INSERT OR REPLACE INTO operations(id,scan_id,kind,started,finished,payload) VALUES(?1,?2,?3,?4,?4,?5)",params![id,scan_id,kind,now(),serde_json::to_string(results)?])?;
        Ok(())
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Payload {
    node: Node,
    fingerprint: Fingerprint,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct CompactPayload {
    v: u8,
    metadata: platform::Metadata,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct TinyPayload {
    v: u8,
    b: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    f: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    t: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    a: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    i: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    n: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    l: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    g: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    c: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    m: Option<String>,
}
fn is_zero(n: &u32) -> bool {
    *n == 0
}
impl TinyPayload {
    fn from_meta(m: &platform::Metadata) -> Self {
        Self {
            v: 3,
            b: m.logical,
            f: m.attributes,
            t: m.reparse_tag,
            a: m.allocated,
            i: m.identity.clone(),
            n: m.links,
            l: m.link_type.clone(),
            g: m.link_target.clone(),
            c: m.cloud_state.clone(),
            m: m.allocation_method.clone(),
        }
    }
}
pub fn read_node(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredNode> {
    let text: String = r.get(3)?;
    let path = platform::decode_path(r.get(2)?);
    let (mut node, fingerprint) = if text.starts_with("{\"v\":3") || text.starts_with("{\"v\":2") {
        let m = if text.starts_with("{\"v\":3") {
            let p: TinyPayload = serde_json::from_str(&text).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            platform::Metadata {
                kind: r.get(15)?,
                logical: p.b,
                allocated: p.a,
                modified: r.get(17)?,
                identity: p.i,
                links: p.n,
                attributes: p.f,
                reparse_tag: p.t,
                link_type: p.l,
                link_target: p.g,
                cloud_state: p.c,
                allocation_method: p.m,
                recurse: false,
            }
        } else {
            let p: CompactPayload = serde_json::from_str(&text).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            p.metadata
        };
        let fp = m.fingerprint();
        let node = Node {
            node_id: String::new(),
            parent_id: None,
            name: r.get(14)?,
            display_path: r.get(18)?,
            kind: m.kind.clone(),
            extension: r.get(16)?,
            depth: 0,
            logical_bytes: m.logical.to_string(),
            logical_total_bytes: String::new(),
            allocated_bytes: m.allocated.map(|b| b.to_string()),
            allocated_known_total_bytes: String::new(),
            allocated_unknown_count: String::new(),
            file_count: String::new(),
            directory_count: String::new(),
            modified_at: m.modified,
            identity: m.identity,
            link_count: m.links.map(|n| n.to_string()),
            link_type: m.link_type,
            link_target: m.link_target,
            cloud_state: m.cloud_state,
            allocation_method: m.allocation_method,
            error_code: None,
            incomplete: false,
            stale: false,
            children_count: String::new(),
        };
        (node, fp)
    } else {
        let p: Payload = serde_json::from_str(&text).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
        })?;
        (p.node, p.fingerprint)
    };
    node.node_id = r.get::<_, i64>(0)?.to_string();
    node.parent_id = r.get::<_, Option<i64>>(1)?.map(|v| v.to_string());
    node.logical_total_bytes = r.get::<_, i64>(4)?.to_string();
    node.allocated_known_total_bytes = r.get::<_, i64>(5)?.to_string();
    node.allocated_unknown_count = r.get::<_, i64>(6)?.to_string();
    node.file_count = r.get::<_, i64>(7)?.to_string();
    node.directory_count = r.get::<_, i64>(8)?.to_string();
    node.children_count = r.get::<_, i64>(9)?.to_string();
    node.incomplete = r.get(10)?;
    node.stale = r.get(11)?;
    node.error_code = r.get(12)?;
    node.depth = r.get(13)?;
    Ok(StoredNode {
        node,
        path,
        fingerprint,
    })
}
pub fn checked_add(a: i64, b: i64) -> Result<i64> {
    a.checked_add(b)
        .filter(|v| *v >= 0)
        .ok_or_else(|| AppError::new("SIZE_OUT_OF_RANGE", "扫描合计超过支持的精确整数范围"))
}
// Explicit SQL column inputs keep identity/path/aggregation choices visible.
#[allow(clippy::too_many_arguments)]
pub fn insert_node(
    tx: &Transaction<'_>,
    scan_id: &str,
    id: i64,
    parent: Option<i64>,
    path: &Path,
    depth: u32,
    m: &platform::Metadata,
    error: Option<&AppError>,
) -> Result<()> {
    insert_node_inner(tx, scan_id, id, parent, path, depth, m, error, true)
}
#[allow(clippy::too_many_arguments)] // Same column contract, batch aggregation.
pub fn insert_node_shallow(
    tx: &Transaction<'_>,
    scan_id: &str,
    id: i64,
    parent: Option<i64>,
    path: &Path,
    depth: u32,
    m: &platform::Metadata,
    error: Option<&AppError>,
) -> Result<()> {
    insert_node_inner(tx, scan_id, id, parent, path, depth, m, error, false)
}

#[derive(Default)]
pub struct InsertTimings {
    pub encode_ms: f64,
    pub sql_ms: f64,
    pub pending_ms: f64,
}

/// Complete derived fields, prepared without SQLite ownership. Raw UTF-16 is
/// unchanged; the display and search strings remain separate from that path.
pub struct EncodedNode {
    raw_path: Vec<u8>,
    payload: String,
    name: String,
    extension: String,
    display_path: String,
}
impl EncodedNode {
    pub fn bytes(&self) -> usize {
        self.raw_path.capacity()
            + self.payload.capacity()
            + self.name.capacity()
            + self.extension.capacity()
            + self.display_path.capacity()
    }
}
pub fn encode_node(path: &Path, m: &platform::Metadata) -> Result<EncodedNode> {
    Ok(EncodedNode {
        raw_path: platform::encode_path(path),
        payload: serde_json::to_string(&TinyPayload::from_meta(m))?,
        name: path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| platform::display(path)),
        extension: if m.kind == "file" {
            path.extension()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        } else {
            String::new()
        },
        display_path: platform::display(path),
    })
}

/// A batch owns its prepared statements until every row has been bound. This
/// avoids a cache lookup/reset and return for every discovered file, while
/// retaining the same columns, constraints, indexes and transaction boundary.
pub struct NodeInserter<'a> {
    insert: rusqlite::CachedStatement<'a>,
    pending: rusqlite::CachedStatement<'a>,
    metadata_pending: rusqlite::CachedStatement<'a>,
    pub timings: Option<InsertTimings>,
}
impl<'a> NodeInserter<'a> {
    pub fn new(tx: &'a Transaction<'_>, profile: bool) -> Result<Self> {
        Ok(Self {
            insert: tx.prepare_cached("INSERT INTO nodes(scan_id,id,parent_id,path,payload,name,kind,extension,depth,modified,logical_total,allocated_total,allocated_unknown,file_count,dir_count,error_code,search_path,incomplete) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,0,?15,?16,?17)")?,
            pending: tx.prepare_cached("INSERT INTO pending_dirs(scan_id,id,path,depth,expected_identity,metadata_deferred) VALUES(?1,?2,?3,?4,?5,?6)")?,
            metadata_pending: tx.prepare_cached("INSERT INTO directory_metadata_pending(scan_id,id) VALUES(?1,?2)")?,
            timings: profile.then(InsertTimings::default),
        })
    }
    #[allow(clippy::too_many_arguments)] // The persisted node's column contract.
    pub fn insert(
        &mut self,
        scan_id: &str,
        id: i64,
        parent: Option<i64>,
        path: &Path,
        depth: u32,
        m: &platform::Metadata,
        error: Option<&AppError>,
    ) -> Result<()> {
        let started = self.timings.as_ref().map(|_| std::time::Instant::now());
        let encoded = encode_node(path, m)?;
        if let (Some(timings), Some(started)) = (&mut self.timings, started) {
            timings.encode_ms += started.elapsed().as_secs_f64() * 1000.0;
        }
        self.insert_encoded(scan_id, id, parent, depth, m, error, &encoded)
    }
    #[allow(clippy::too_many_arguments)] // Same persisted node, precomputed fields.
    pub fn insert_encoded(
        &mut self,
        scan_id: &str,
        id: i64,
        parent: Option<i64>,
        depth: u32,
        m: &platform::Metadata,
        error: Option<&AppError>,
        encoded: &EncodedNode,
    ) -> Result<()> {
        let logical = integer(m.logical)?;
        let allocated = m.allocated.map(integer).transpose()?.unwrap_or(0);
        let isfile = m.kind == "file";
        let started = self.timings.as_ref().map(|_| std::time::Instant::now());
        self.insert.execute(params![
            scan_id,
            id,
            parent,
            encoded.raw_path,
            encoded.payload,
            encoded.name,
            m.kind,
            encoded.extension,
            depth,
            m.modified,
            logical,
            allocated,
            m.allocated.is_none(),
            isfile,
            error.map(|e| &e.code),
            encoded.display_path,
            if error.is_some() {
                1i64
            } else if m.recurse {
                2
            } else {
                0
            }
        ])?;
        if let (Some(timings), Some(started)) = (&mut self.timings, started) {
            timings.sql_ms += started.elapsed().as_secs_f64() * 1000.0;
        }
        if m.recurse {
            let started = self.timings.as_ref().map(|_| std::time::Instant::now());
            self.pending.execute(params![
                scan_id,
                id,
                encoded.raw_path,
                depth,
                m.identity,
                m.defer_directory_metadata()
            ])?;
            if m.defer_directory_metadata() {
                self.metadata_pending.execute(params![scan_id, id])?;
            }
            if let (Some(timings), Some(started)) = (&mut self.timings, started) {
                timings.pending_ms += started.elapsed().as_secs_f64() * 1000.0;
            }
        }
        Ok(())
    }
}
pub struct DirectoryMetadataUpdate {
    pub id: i64,
    pub expected_identity: String,
    pub result: Result<platform::Metadata>,
}
impl DirectoryMetadataUpdate {
    pub fn bytes(&self) -> usize {
        self.expected_identity.capacity()
            + self.result.as_ref().map_or_else(
                |e| {
                    e.code.capacity()
                        + e.message.capacity()
                        + e.node_id.as_ref().map_or(0, String::capacity)
                },
                |m| {
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
                        .map(|s| s.as_ref().map_or(0, String::capacity))
                        .sum::<usize>()
                },
            )
    }
}
pub fn check_directory_metadata_complete(c: &Connection, scan_id: &str) -> Result<()> {
    let pending: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM directory_metadata_pending WHERE scan_id=?1)",
        [scan_id],
        |r| r.get(0),
    )?;
    if pending {
        Err(AppError::new(
            "METADATA_INCOMPLETE",
            "扫描读取已结束，但目录元数据尚未完整回写；未标记完成",
        ))
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod directory_metadata_tests {
    use super::*;
    use crate::service::Service;
    fn setup() -> (
        tempfile::TempDir,
        std::sync::Arc<Service>,
        String,
        PathBuf,
        platform::Metadata,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("source");
        std::fs::create_dir_all(root.join("child")).unwrap();
        let root = platform::absolute(&root).unwrap();
        let child = root.join("child");
        let service = Service::new(&temp.path().join("data")).unwrap();
        let scan = service.repo.new_scan_id().unwrap();
        service.repo.insert_scan(&scan, &root, false).unwrap();
        let complete = platform::metadata(&child).unwrap();
        let mut partial = complete.clone();
        partial.links = None;
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
        insert_node(&tx, &scan, 2, Some(1), &child, 1, &partial, None).unwrap();
        tx.commit().unwrap();
        (temp, service, scan, child, complete)
    }
    #[test]
    fn equal_timestamp_completion_avoids_secondary_writes_and_changed_timestamp_sorts_exactly() {
        let (_temp, service, scan, child, complete) = setup();
        let mut c = service.repo.connect().unwrap();
        let opcodes = |sql: &str| {
            let mut stmt = c.prepare(&format!("EXPLAIN {sql}")).unwrap();
            let count = stmt.parameter_count();
            let codes = stmt
                .query_map(
                    rusqlite::params_from_iter(std::iter::repeat_n(
                        rusqlite::types::Value::Null,
                        count,
                    )),
                    |r| {
                        Ok((
                            r.get::<_, String>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, i64>(3)?,
                        ))
                    },
                )
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap();
            codes
        };
        let payload_only = opcodes(DIRECTORY_PAYLOAD_UPDATE);
        let timestamp = opcodes(DIRECTORY_TIMED_PAYLOAD_UPDATE);
        let secondary_root: i64 = c
            .query_row(
                "SELECT rootpage FROM sqlite_schema WHERE name='nodes_special_kind_v2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // WITHOUT ROWID uses IdxInsert for the primary table too. Identify the
        // actual secondary cursor instead of mistaking that table write for it.
        assert!(
            !payload_only
                .iter()
                .any(|(op, _, page)| op == "OpenWrite" && *page == secondary_root),
            "SQLite {} {payload_only:?}",
            rusqlite::version()
        );
        let secondary_cursor = timestamp
            .iter()
            .find(|(op, _, page)| op == "OpenWrite" && *page == secondary_root)
            .unwrap()
            .1;
        assert!(timestamp
            .iter()
            .any(|(op, cursor, _)| op == "IdxDelete" && *cursor == secondary_cursor));
        assert!(timestamp
            .iter()
            .any(|(op, cursor, _)| op == "IdxInsert" && *cursor == secondary_cursor));
        let tx = c.transaction().unwrap();
        let mut comparison = complete.clone();
        comparison.modified = Some("2020-01-01T00:00:00+00:00".into());
        comparison.identity = Some("comparison-fixture-identity".into());
        insert_node(
            &tx,
            &scan,
            3,
            Some(1),
            &child.with_file_name("comparison"),
            1,
            &comparison,
            None,
        )
        .unwrap();
        aggregate_batch(&tx, &scan, 1, 0, 0, 0, 0, 2, 2).unwrap();
        tx.commit().unwrap();
        let before = service.repo.scan(&scan).unwrap();
        for (index, modified) in [
            complete.modified.clone(),
            Some("2030-01-01T00:00:00+00:00".into()),
            None,
            None,
        ]
        .into_iter()
        .enumerate()
        {
            let mut metadata = complete.clone();
            metadata.modified = modified.clone();
            metadata.links = Some(7 + index as u64);
            metadata.attributes |= 1;
            let tx = c.transaction().unwrap();
            apply_directory_metadata(
                &tx,
                &scan,
                &DirectoryMetadataUpdate {
                    id: 2,
                    expected_identity: complete.identity.clone().unwrap(),
                    result: Ok(metadata.clone()),
                },
            )
            .unwrap();
            let saved = Repository::node_conn(&tx, &scan, 2).unwrap();
            assert_eq!(saved.node.modified_at, modified);
            assert_eq!(saved.node.link_count, metadata.links.map(|n| n.to_string()));
            assert_eq!(saved.fingerprint.attributes, metadata.attributes);
            assert_eq!(saved.node.identity, metadata.identity);
            tx.commit().unwrap();
            if index == 1 {
                let page = service
                    .repo
                    .page(
                        &PageRequest {
                            scan_id: scan.clone(),
                            revision: before.revision,
                            parent_id: Some("1".into()),
                            sort: Some("modified_desc".into()),
                            filter: None,
                            cursor: None,
                            page_size: Some(200),
                        },
                        false,
                    )
                    .unwrap();
                assert_eq!(page.nodes[0].node_id, "2");
                assert_eq!(page.nodes[1].node_id, "3");
            }
        }
        let after = service.repo.scan(&scan).unwrap();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.discovered_count, before.discovered_count);
        assert_eq!(after.logical_total_bytes, before.logical_total_bytes);
        check_directory_metadata_complete(&c, &scan).unwrap();
    }
    #[test]
    fn saved_and_handle_identity_checks_precede_atomic_metadata_completion() {
        let (_temp, service, scan, _child, complete) = setup();
        let mut c = service.repo.connect().unwrap();
        let expected = complete.identity.clone().unwrap();
        assert_eq!(
            check_directory_metadata_complete(&c, &scan)
                .unwrap_err()
                .code,
            "METADATA_INCOMPLETE"
        );
        let before = service.repo.scan(&scan).unwrap();
        let tx = c.transaction().unwrap();
        let mut replacement = complete.clone();
        replacement.identity = Some("replacement".into());
        assert_eq!(
            apply_directory_metadata(
                &tx,
                &scan,
                &DirectoryMetadataUpdate {
                    id: 2,
                    expected_identity: expected.clone(),
                    result: Ok(replacement)
                }
            )
            .unwrap_err()
            .code,
            "SOURCE_CHANGED"
        );
        assert!(Repository::node_conn(&tx, &scan, 2)
            .unwrap()
            .node
            .link_count
            .is_none());
        apply_directory_metadata(
            &tx,
            &scan,
            &DirectoryMetadataUpdate {
                id: 2,
                expected_identity: expected.clone(),
                result: Ok(complete.clone()),
            },
        )
        .unwrap();
        tx.execute("UPDATE scans SET revision=revision+1 WHERE id=?1", [&scan])
            .unwrap();
        assert_eq!(
            Repository::node_conn(&tx, &scan, 2)
                .unwrap()
                .node
                .link_count,
            complete.links.map(|v| v.to_string())
        );
        assert!(service
            .repo
            .node(&scan, "2", None)
            .unwrap()
            .node
            .link_count
            .is_none());
        assert_eq!(service.repo.scan(&scan).unwrap().revision, before.revision);
        tx.commit().unwrap();
        check_directory_metadata_complete(&c, &scan).unwrap();
        let after = service.repo.scan(&scan).unwrap();
        assert_eq!(after.revision, before.revision + 1);
        assert_eq!(after.discovered_count, before.discovered_count);
        assert_eq!(after.logical_total_bytes, before.logical_total_bytes);
        let tx = c.transaction().unwrap();
        tx.execute("UPDATE nodes SET payload=json_set(payload,'$.i','new-row-identity') WHERE scan_id=?1 AND id=2",[&scan]).unwrap();
        assert_eq!(
            apply_directory_metadata(
                &tx,
                &scan,
                &DirectoryMetadataUpdate {
                    id: 2,
                    expected_identity: expected,
                    result: Ok(complete)
                }
            )
            .unwrap_err()
            .code,
            "SOURCE_CHANGED"
        );
        assert_eq!(
            Repository::node_conn(&tx, &scan, 2)
                .unwrap()
                .node
                .identity
                .as_deref(),
            Some("new-row-identity")
        );
        tx.rollback().unwrap();
    }
    #[test]
    fn public_detail_payload_is_compatible_with_later_same_handle_completion() {
        let (_temp, service, scan, child, _complete) = setup();
        let revision = service.repo.scan(&scan).unwrap().revision;
        let (measured, next) = service.measure_node(&scan, "2", revision).unwrap();
        assert!(next > revision);
        assert!(measured.link_count.is_some());
        let fresh = platform::ScanDirectoryReader::new(child.parent().unwrap(), true)
            .prepare(&child)
            .unwrap()
            .directory_metadata()
            .unwrap();
        let mut c = service.repo.connect().unwrap();
        let tx = c.transaction().unwrap();
        apply_directory_metadata(
            &tx,
            &scan,
            &DirectoryMetadataUpdate {
                id: 2,
                expected_identity: fresh.identity.clone().unwrap(),
                result: Ok(fresh.clone()),
            },
        )
        .unwrap();
        tx.commit().unwrap();
        check_directory_metadata_complete(&c, &scan).unwrap();
        let saved = service.repo.node(&scan, "2", Some(next)).unwrap();
        assert_eq!(saved.node.link_count, fresh.links.map(|v| v.to_string()));
        assert_eq!(saved.node.identity, fresh.identity);
        assert_eq!(service.repo.scan(&scan).unwrap().revision, next);
    }
    #[test]
    fn failed_completion_keeps_known_observations_and_marks_unknown_partial() {
        let (_temp, service, scan, _child, complete) = setup();
        let mut c = service.repo.connect().unwrap();
        let tx = c.transaction().unwrap();
        apply_directory_metadata(
            &tx,
            &scan,
            &DirectoryMetadataUpdate {
                id: 2,
                expected_identity: complete.identity.clone().unwrap(),
                result: Err(AppError::new("ACCESS_DENIED", "fixture denied")),
            },
        )
        .unwrap();
        tx.commit().unwrap();
        check_directory_metadata_complete(&c, &scan).unwrap();
        let saved = service.repo.node(&scan, "2", None).unwrap();
        assert_eq!(saved.node.identity, complete.identity);
        assert_eq!(saved.node.kind, "directory");
        assert_eq!(saved.node.allocated_bytes, Some("0".into()));
        assert!(saved.node.link_count.is_none());
        assert_eq!(saved.node.error_code.as_deref(), Some("ACCESS_DENIED"));
        assert!(saved.node.incomplete);
        assert_eq!(service.repo.scan(&scan).unwrap().error_count, "1");
    }
}
/// Applies only metadata captured on the directory's validated handle. This
/// function shares the caller's discovery/aggregation/revision transaction.
const DIRECTORY_PAYLOAD_UPDATE: &str = "UPDATE nodes SET payload=?3 WHERE scan_id=?1 AND id=?2";
const DIRECTORY_TIMED_PAYLOAD_UPDATE: &str =
    "UPDATE nodes SET payload=?3,modified=?4 WHERE scan_id=?1 AND id=?2";
pub fn apply_directory_metadata(
    tx: &Transaction<'_>,
    scan_id: &str,
    update: &DirectoryMetadataUpdate,
) -> Result<()> {
    let mut select =
        tx.prepare_cached("SELECT kind,payload,modified FROM nodes WHERE scan_id=?1 AND id=?2")?;
    let (kind, payload, modified): (String, String, Option<String>) = select
        .query_row(params![scan_id, update.id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    drop(select);
    let (identity, logical, allocated) = if payload.starts_with("{\"v\":3") {
        let old: TinyPayload = serde_json::from_str(&payload)?;
        (old.i, old.b, old.a)
    } else {
        // Public detail measurement can replace the compact payload while this
        // directory is queued. Preserve v1/v2/v3 payload and revision semantics.
        let old = Repository::node_conn(tx, scan_id, update.id)?;
        (
            old.fingerprint.identity,
            old.fingerprint.logical,
            old.node
                .allocated_bytes
                .as_deref()
                .map(parse_bytes)
                .transpose()?
                .map(|v| v as u64),
        )
    };
    if kind != "directory"
        || identity.as_deref() != Some(&update.expected_identity)
        || logical != 0
        || allocated != Some(0)
    {
        return Err(AppError::new(
            "SOURCE_CHANGED",
            "待补目录记录的原身份或统计语义已改变，未覆盖元数据",
        ));
    }
    let mut issue = None;
    match &update.result {
        Ok(m)
            if m.kind == "directory"
                && m.recurse
                && m.identity.as_deref() == Some(&update.expected_identity)
                && m.logical == 0
                && m.allocated == Some(0) =>
        {
            if m.links.is_none() {
                issue = Some(AppError::new(
                    "METADATA_UNAVAILABLE",
                    "目录链接数未能可靠读取，保留未知值",
                ));
            }
            // Directory-own allocation is zero under the existing accounting
            // policy. No aggregate/discovered/direct-child value changes here.
            let payload = serde_json::to_string(&TinyPayload::from_meta(m))?;
            if m.modified == modified {
                // SQLite maintains indexes for a SET column even if its value
                // is identical. The unchanged timestamp stays exact without
                // rewriting the directory's secondary-index entry.
                tx.prepare_cached(DIRECTORY_PAYLOAD_UPDATE)?
                    .execute(params![scan_id, update.id, payload])?;
            } else {
                tx.prepare_cached(DIRECTORY_TIMED_PAYLOAD_UPDATE)?
                    .execute(params![scan_id, update.id, payload, m.modified])?;
            }
        }
        Ok(_) => {
            return Err(AppError::new(
                "SOURCE_CHANGED",
                "目录句柄身份与原记录不同，未覆盖元数据",
            ))
        }
        Err(error) => {
            // The parent enumeration already proved this directory's identity,
            // attributes and accounting type. Failed completion preserves those
            // known observations and the explicitly unknown link count.
            issue = Some(error.clone());
        }
    }
    if let Some(error) = issue {
        let stored = Repository::node_conn(tx, scan_id, update.id)?;
        Repository::error_conn(
            tx,
            scan_id,
            ScanError {
                node_id: Some(update.id.to_string()),
                display_path: platform::display(&stored.path),
                stage: "metadata".into(),
                error,
            },
        )?;
    }
    tx.prepare_cached("DELETE FROM directory_metadata_pending WHERE scan_id=?1 AND id=?2")?
        .execute(params![scan_id, update.id])?;
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Independently checked integer aggregates.
pub fn aggregate_batch(
    tx: &Transaction<'_>,
    scan_id: &str,
    parent: i64,
    logical: i64,
    allocated: i64,
    unknown: i64,
    files: i64,
    dirs: i64,
    children: i64,
) -> Result<()> {
    let sql="WITH RECURSIVE ancestors(id) AS (SELECT ?2 UNION ALL SELECT n.parent_id FROM ancestors a CROSS JOIN nodes n ON n.scan_id=?1 AND n.id=a.id WHERE n.parent_id IS NOT NULL) UPDATE nodes SET logical_total=logical_total+?3,allocated_total=allocated_total+?4,allocated_unknown=allocated_unknown+?5,file_count=file_count+?6,dir_count=dir_count+?7 WHERE scan_id=?1 AND id IN ancestors";
    tx.execute(sql,params![scan_id,parent,logical,allocated,unknown,files,dirs]).map_err(|error|if matches!(error,rusqlite::Error::SqliteFailure(ref e,_) if e.extended_code==rusqlite::ffi::SQLITE_CONSTRAINT_CHECK){AppError::new("SIZE_OUT_OF_RANGE","扫描总量超出精确整数支持范围")}else{error.into()})?;
    tx.execute(
        "UPDATE nodes SET children_count=children_count+?3 WHERE scan_id=?1 AND id=?2",
        params![scan_id, parent, children],
    )?;
    tx.execute(
        "UPDATE scans SET discovered=discovered+?2 WHERE id=?1",
        params![scan_id, children],
    )?;
    Ok(())
}

#[derive(Debug, Default)]
pub struct AggregateStats {
    pub direct_parents: usize,
    pub ancestor_rows: usize,
    pub coalesced: bool,
}
#[derive(Default)]
struct AggregateRow {
    parent: Option<i64>,
    current: [i64; 6],
    delta: [i64; 6],
}
/// Coalesce shared ancestors once per committed batch. The maps contain only
/// fixed-size IDs/counters, no paths or metadata. At most 32,768 ancestor rows
/// are retained; an unusually dispersed/deep batch uses the original bounded
/// per-parent SQL without partially applying the coalesced representation.
pub fn aggregate_many(
    tx: &Transaction<'_>,
    scan_id: &str,
    direct: &std::collections::BTreeMap<i64, [i64; 6]>,
) -> Result<AggregateStats> {
    aggregate_many_with_limit(tx, scan_id, direct, 32_768)
}
fn aggregate_many_with_limit(
    tx: &Transaction<'_>,
    scan_id: &str,
    direct: &std::collections::BTreeMap<i64, [i64; 6]>,
    limit: usize,
) -> Result<AggregateStats> {
    fn add_delta(a: i64, b: i64) -> Result<i64> {
        a.checked_add(b)
            .ok_or_else(|| AppError::new("SIZE_OUT_OF_RANGE", "扫描增量超过支持的精确整数范围"))
    }
    let mut rows = std::collections::BTreeMap::<i64, AggregateRow>::new();
    let mut load = tx.prepare_cached("SELECT parent_id,logical_total,allocated_total,allocated_unknown,file_count,dir_count,children_count FROM nodes WHERE scan_id=?1 AND id=?2")?;
    for parent in direct.keys() {
        let mut next = Some(*parent);
        let mut seen = HashSet::new();
        while let Some(id) = next {
            if !seen.insert(id) {
                return Err(AppError::new("CORRUPT_HIERARCHY", "扫描层级记录损坏"));
            }
            if seen.len() > 4096 {
                return Err(AppError::new("DEPTH_LIMIT", "目录深度超过支持上限"));
            }
            if rows.contains_key(&id) {
                break;
            }
            if rows.len() >= limit {
                drop(load);
                drop(rows);
                for (parent, delta) in direct {
                    aggregate_batch(
                        tx, scan_id, *parent, delta[0], delta[1], delta[2], delta[3], delta[4],
                        delta[5],
                    )?;
                }
                return Ok(AggregateStats {
                    direct_parents: direct.len(),
                    ancestor_rows: 0,
                    coalesced: false,
                });
            }
            let row = load
                .query_row(params![scan_id, id], |r| {
                    Ok(AggregateRow {
                        parent: r.get(0)?,
                        current: [
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                            r.get(6)?,
                        ],
                        ..Default::default()
                    })
                })
                .optional()?
                .ok_or_else(|| AppError::new("NODE_NOT_FOUND", "聚合父目录已不存在"))?;
            next = row.parent;
            rows.insert(id, row);
        }
    }
    drop(load);
    let mut discovered_delta = 0i64;
    for (id, delta) in direct {
        rows.get_mut(id).unwrap().delta = *delta;
        discovered_delta = add_delta(discovered_delta, delta[5])?;
    }
    // Traverse the actual parent graph, rather than trusting depth or discovery
    // ID order (which can differ after a scoped refresh or legacy import).
    let mut unfinished = rows
        .keys()
        .map(|id| (*id, 0usize))
        .collect::<std::collections::BTreeMap<_, _>>();
    for row in rows.values() {
        if let Some(parent) = row.parent {
            *unfinished.get_mut(&parent).unwrap() += 1;
        }
    }
    let mut ready = unfinished
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| *id)
        .collect::<std::collections::VecDeque<_>>();
    let mut visited = 0usize;
    while let Some(id) = ready.pop_front() {
        visited += 1;
        let row = &rows[&id];
        let parent = row.parent;
        let delta = row.delta;
        if let Some(parent) = parent {
            let ancestor = rows.get_mut(&parent).unwrap();
            for (total, change) in ancestor.delta[..5].iter_mut().zip(delta[..5].iter()) {
                *total = add_delta(*total, *change)?;
            }
            let count = unfinished.get_mut(&parent).unwrap();
            *count -= 1;
            if *count == 0 {
                ready.push_back(parent);
            }
        }
    }
    if visited != rows.len() {
        return Err(AppError::new("CORRUPT_HIERARCHY", "扫描层级记录损坏"));
    }
    let discovered: i64 = tx.query_row(
        "SELECT discovered FROM scans WHERE id=?1",
        [scan_id],
        |row| row.get(0),
    )?;
    let discovered = checked_add(discovered, discovered_delta)?;
    if discovered < 0 {
        return Err(AppError::new("SIZE_OUT_OF_RANGE", "扫描计数超出支持范围"));
    }
    for row in rows.values_mut() {
        for (current, delta) in row.current.iter_mut().zip(row.delta.iter()) {
            *current = checked_add(*current, *delta)?;
            if *current < 0 {
                return Err(AppError::new("SIZE_OUT_OF_RANGE", "扫描统计不能为负数"));
            }
        }
    }
    // All sums are checked before the first UPDATE. The caller's transaction
    // also contains nodes, direct child counts, queue consumption and revision.
    let mut update = tx.prepare_cached("UPDATE nodes SET logical_total=?3,allocated_total=?4,allocated_unknown=?5,file_count=?6,dir_count=?7,children_count=?8 WHERE scan_id=?1 AND id=?2")?;
    for (id, row) in &rows {
        update.execute(params![
            scan_id,
            id,
            row.current[0],
            row.current[1],
            row.current[2],
            row.current[3],
            row.current[4],
            row.current[5]
        ])?;
    }
    drop(update);
    tx.execute(
        "UPDATE scans SET discovered=?2 WHERE id=?1",
        params![scan_id, discovered],
    )?;
    Ok(AggregateStats {
        direct_parents: direct.len(),
        ancestor_rows: rows.len(),
        coalesced: true,
    })
}
#[allow(clippy::too_many_arguments)] // Shared insertion primitive for both modes.
fn insert_node_inner(
    tx: &Transaction<'_>,
    scan_id: &str,
    id: i64,
    parent: Option<i64>,
    path: &Path,
    depth: u32,
    m: &platform::Metadata,
    error: Option<&AppError>,
    aggregate: bool,
) -> Result<()> {
    let logical = integer(m.logical)?;
    let allocated = m.allocated.map(integer).transpose()?.unwrap_or(0);
    let isfile = m.kind == "file";
    let mut inserter = NodeInserter::new(tx, false)?;
    inserter.insert(scan_id, id, parent, path, depth, m, error)?;
    drop(inserter);
    if aggregate {
        if let Some(parent) = parent {
            let parents = Repository::ancestors_conn(tx, scan_id, id)?;
            for ancestor in parents {
                let aid = parse_id(&ancestor.node.node_id)?;
                checked_add(parse_bytes(&ancestor.node.logical_total_bytes)?, logical)?;
                checked_add(
                    parse_bytes(&ancestor.node.allocated_known_total_bytes)?,
                    allocated,
                )?;
                tx.execute("UPDATE nodes SET logical_total=logical_total+?3,allocated_total=allocated_total+?4,allocated_unknown=allocated_unknown+?5,file_count=file_count+?6,dir_count=dir_count+?7 WHERE scan_id=?1 AND id=?2",params![scan_id,aid,logical,allocated,m.allocated.is_none(),isfile,m.kind=="directory"])?;
            }
            tx.execute(
                "UPDATE nodes SET children_count=children_count+1 WHERE scan_id=?1 AND id=?2",
                params![scan_id, parent],
            )?;
        }
        tx.execute(
            "UPDATE scans SET discovered=discovered+1 WHERE id=?1",
            [scan_id],
        )?;
    }
    Ok(())
}
fn chart_node(
    c: &Connection,
    scan_id: &str,
    id: i64,
    depth: u32,
    left: &mut u32,
    truncated: &mut bool,
    mut view_budget: Option<&mut ViewBudget>,
) -> Result<ChartNode> {
    let node = Repository::node_conn(c, scan_id, id)?.node;
    *left = left.saturating_sub(1);
    let mut out = ChartNode {
        node,
        children: vec![],
        other_count: "0".into(),
        other_bytes: "0".into(),
    };
    if let Some(budget) = &mut view_budget {
        budget.bytes = budget.bytes.saturating_add(200); // Per-node chart envelopes/counters.
        budget.charge(&out.node, 1)?;
    }
    if depth == 0 || *left == 0 {
        out.other_count = out.node.children_count.clone();
        out.other_bytes = out.node.logical_total_bytes.clone();
        if *left == 0 && out.other_count != "0" {
            *truncated = true;
        }
        return Ok(out);
    }
    let limit = (*left).min(200);
    // Queue IDs, not a second full path/metadata copy for each child. Each
    // recursive node is decoded once and budgeted before its descendants.
    let sql="SELECT id FROM nodes WHERE scan_id=?1 AND parent_id=?2 ORDER BY sort_size,name COLLATE NOCASE,id LIMIT ?3";
    let mut stmt = c.prepare(sql)?;
    let direct = stmt
        .query_map(params![scan_id, id, limit], |row| row.get::<_, i64>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut known = 0i64;
    let mut count = 0u64;
    for child in direct {
        if *left == 0 {
            break;
        }
        let child = chart_node(
            c,
            scan_id,
            child,
            depth - 1,
            left,
            truncated,
            view_budget.as_deref_mut(),
        )?;
        known = checked_add(known, parse_bytes(&child.node.logical_total_bytes)?)?;
        count += 1;
        out.children.push(child);
    }
    let total_count = out.node.children_count.parse::<u64>().unwrap_or(0);
    out.other_count = total_count.saturating_sub(count).to_string();
    out.other_bytes = parse_bytes(&out.node.logical_total_bytes)?
        .saturating_sub(known)
        .to_string();
    if total_count > count {
        *truncated = true;
    }
    Ok(out)
}

#[cfg(test)]
mod scan_storage_tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Repository, PathBuf, String) {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("source");
        std::fs::create_dir_all(root.join("bad")).unwrap();
        std::fs::create_dir_all(root.join("good")).unwrap();
        std::fs::write(root.join("bad/file.bin"), [1u8; 7]).unwrap();
        std::fs::write(root.join("good/file.bin"), [2u8; 11]).unwrap();
        let root = platform::absolute(&root).unwrap();
        let repo = Repository::new(&fixture.path().join("db")).unwrap();
        let scan_id = repo.new_scan_id().unwrap();
        repo.insert_scan(&scan_id, &root, false).unwrap();
        let mut c = repo.connect().unwrap();
        let tx = c.transaction().unwrap();
        for (id, parent, name, depth) in [
            (1, None, "", 0),
            (2, Some(1), "bad", 1),
            (3, Some(2), "bad/file.bin", 2),
            (4, Some(1), "good", 1),
            (5, Some(4), "good/file.bin", 2),
        ] {
            let path = root.join(name);
            insert_node(
                &tx,
                &scan_id,
                id,
                parent,
                &path,
                depth,
                &platform::metadata(&path).unwrap(),
                None,
            )
            .unwrap();
        }
        tx.commit().unwrap();
        (fixture, repo, root, scan_id)
    }

    #[test]
    fn queue_upgrade_preserves_saved_nodes_and_interrupts_active_work() {
        for suffix in ["", " WITHOUT ROWID"] {
            let (fixture, repo, root, saved_id) = fixture();
            let saved = repo.finish(&saved_id, "completed").unwrap();
            let saved_node = repo.node(&saved_id, "2", None).unwrap().node;
            let active_id = repo.new_scan_id().unwrap();
            repo.insert_scan(&active_id, &root, false).unwrap();
            let c = repo.connect().unwrap();
            c.execute_batch(&format!("DROP TABLE pending_dirs; CREATE TABLE pending_dirs(scan_id TEXT NOT NULL,id INTEGER NOT NULL,path BLOB NOT NULL,depth INTEGER NOT NULL,PRIMARY KEY(scan_id,id)){suffix};")).unwrap();
            c.execute(
                "INSERT INTO pending_dirs VALUES(?1,1,?2,0)",
                params![active_id, platform::encode_path(&root)],
            )
            .unwrap();
            c.execute(
                "UPDATE scans SET status='running' WHERE id=?1",
                [&active_id],
            )
            .unwrap();
            drop(c);
            drop(repo);

            let repo = Repository::new(&fixture.path().join("db")).unwrap();
            assert_eq!(
                serde_json::to_value(repo.scan(&saved_id).unwrap()).unwrap(),
                serde_json::to_value(saved).unwrap()
            );
            assert_eq!(
                serde_json::to_value(repo.node(&saved_id, "2", None).unwrap().node).unwrap(),
                serde_json::to_value(saved_node).unwrap()
            );
            let recovered = repo.scan(&active_id).unwrap();
            assert_eq!(recovered.status, "interrupted");
            assert!(recovered.stale && recovered.incomplete);
            let c = repo.connect().unwrap();
            assert!(c
                .query_row::<bool, _, _>(
                    "SELECT wr FROM pragma_table_list WHERE schema='main' AND name='pending_dirs'",
                    [],
                    |r| r.get(0)
                )
                .unwrap());
            assert_eq!(
                c.query_row::<i64, _, _>("SELECT COUNT(*) FROM pending_dirs", [], |r| r.get(0))
                    .unwrap(),
                0
            );
            assert!(c.prepare("SELECT rowid FROM pending_dirs").is_err());
            c.execute_batch("CREATE TEMP TABLE consumed_dirs(id INTEGER PRIMARY KEY); INSERT INTO consumed_dirs VALUES(3);").unwrap();
            for (scan_id, id) in [
                (&active_id, 1),
                (&active_id, 2),
                (&active_id, 3),
                (&active_id, 4),
                (&saved_id, 7),
            ] {
                c.execute(
                    "INSERT INTO pending_dirs(scan_id,id,path,depth) VALUES(?1,?2,?3,?2)",
                    params![scan_id, id, platform::encode_path(&root)],
                )
                .unwrap();
            }
            let mut stmt = c.prepare("SELECT id,path,depth FROM pending_dirs WHERE scan_id=?1 AND id NOT IN consumed_dirs ORDER BY id DESC LIMIT ?2").unwrap();
            let result = stmt
                .query_map(params![active_id, 2], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, u32>(2)?,
                    ))
                })
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(result.iter().map(|row| row.0).collect::<Vec<_>>(), [4, 2]);
            assert!(result
                .iter()
                .all(|row| platform::decode_path(row.1.clone()) == root && row.2 == row.0 as u32));
            assert_eq!(c.query_row::<i64,_,_>("SELECT COUNT(*) FROM pending_dirs WHERE expected_identity IS NULL AND metadata_deferred=0", [], |r|r.get(0)).unwrap(),5);
        }
    }

    #[test]
    fn sparse_error_seed_preserves_only_error_branches_on_completion() {
        let (_fixture, repo, root, scan_id) = fixture();
        let other_id = repo.new_scan_id().unwrap();
        repo.insert_scan(&other_id, &root, false).unwrap();
        let mut c = repo.connect().unwrap();
        let tx = c.transaction().unwrap();
        insert_node(
            &tx,
            &other_id,
            1,
            None,
            &root,
            0,
            &platform::metadata(&root).unwrap(),
            None,
        )
        .unwrap();
        Repository::error_conn(
            &tx,
            &scan_id,
            ScanError {
                node_id: Some("3".into()),
                display_path: platform::display(&root.join("bad/file.bin")),
                stage: "enumerate".into(),
                error: AppError::new("ACCESS_DENIED", "fixture"),
            },
        )
        .unwrap();
        tx.commit().unwrap();
        let mut plan = c.prepare("EXPLAIN QUERY PLAN SELECT id FROM nodes INDEXED BY nodes_errors WHERE scan_id=?1 AND error_code IS NOT NULL").unwrap();
        let details = plan
            .query_map([&scan_id], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            details.iter().any(|detail| detail.contains("nodes_errors")),
            "{details:?}"
        );
        let before = repo.scan(&scan_id).unwrap().revision;
        let finished = repo.finish(&scan_id, "completed_with_errors").unwrap();
        assert_eq!(finished.revision, before + 1);
        assert_eq!(finished.error_count, "1");
        for id in ["1", "2", "3"] {
            assert!(repo.node(&scan_id, id, None).unwrap().node.incomplete);
        }
        for id in ["4", "5"] {
            assert!(!repo.node(&scan_id, id, None).unwrap().node.incomplete);
        }
        assert!(repo.node(&other_id, "1", None).unwrap().node.incomplete);
        assert_eq!(
            c.query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM pending_dirs WHERE scan_id=?1",
                [&scan_id],
                |r| r.get(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            c.query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM pending_dirs WHERE scan_id=?1",
                [&other_id],
                |r| r.get(0)
            )
            .unwrap(),
            1
        );
    }
}

#[cfg(test)]
mod view_snapshot_tests {
    use super::*;
    use std::sync::Arc;

    fn fixture() -> (tempfile::TempDir, Arc<Repository>, String) {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("source");
        std::fs::create_dir_all(root.join("folder")).unwrap();
        std::fs::write(root.join("folder/file.bin"), [7u8; 17]).unwrap();
        let root = platform::absolute(&root).unwrap();
        let repo = Arc::new(Repository::new(&fixture.path().join("db")).unwrap());
        let scan_id = repo.new_scan_id().unwrap();
        repo.insert_scan(&scan_id, &root, false).unwrap();
        let mut c = repo.connect().unwrap();
        let tx = c.transaction().unwrap();
        for (id, parent, path, depth) in [
            (1, None, root.clone(), 0),
            (2, Some(1), root.join("folder"), 1),
            (3, Some(2), root.join("folder/file.bin"), 2),
        ] {
            insert_node(
                &tx,
                &scan_id,
                id,
                parent,
                &path,
                depth,
                &platform::metadata(&path).unwrap(),
                None,
            )
            .unwrap();
        }
        tx.commit().unwrap();
        repo.finish(&scan_id, "completed").unwrap();
        (fixture, repo, scan_id)
    }

    fn request(scan_id: &str) -> ViewSnapshotRequest {
        ViewSnapshotRequest {
            scan_id: scan_id.into(),
            scope_id: "2".into(),
            expanded_ids: vec!["1".into(), "2".into()],
            selected_ids: vec!["2".into(), "3".into()],
            primary_id: Some("2".into()),
            sort: Some("name_asc".into()),
            page_anchors: vec![],
        }
    }

    fn assert_consistent(snapshot: &ViewSnapshot, expected: &str) {
        assert_eq!(snapshot.scan.logical_total_bytes, expected);
        assert!(snapshot
            .nodes
            .iter()
            .all(|node| node.logical_total_bytes == expected));
        for page in &snapshot.pages {
            assert_eq!(page.page.revision, snapshot.scan.revision);
            assert!(page
                .page
                .nodes
                .iter()
                .all(|node| node.logical_total_bytes == expected));
        }
        assert_eq!(snapshot.chart.revision, snapshot.scan.revision);
        assert_eq!(snapshot.chart.root.node.logical_total_bytes, expected);
        assert_eq!(
            snapshot.chart.root.children[0].node.logical_total_bytes,
            expected
        );
        assert_eq!(snapshot.summary.logical_total_bytes, expected);
        assert_eq!(snapshot.summary.normalized_ids, ["2"]);
        assert_eq!(snapshot.summary.covered_ids, ["3"]);
        assert_eq!(snapshot.details[0].node_id, "2");
        assert_eq!(snapshot.details[0].nodes[0].logical_bytes, expected);
    }

    #[test]
    fn view_snapshot_releases_reader_before_final_serialization() {
        let (_fixture, repo, scan_id) = fixture();
        let prior_revision = repo.scan(&scan_id).unwrap().revision;
        let (entered_tx, entered_rx) = crossbeam_channel::bounded(1);
        let (release_tx, release_rx) = crossbeam_channel::bounded(1);
        *VIEW_SERIALIZE_HOOK.lock().unwrap() = Some((repo.path.clone(), entered_tx, release_rx));
        let reader_repo = repo.clone();
        let read_request = request(&scan_id);
        let reader = std::thread::spawn(move || reader_repo.view_snapshot(&read_request));
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let writer = repo.connect().unwrap();
        writer
            .execute(
                "UPDATE scans SET revision=revision+1 WHERE id=?1",
                [&scan_id],
            )
            .unwrap();
        let checkpoint = writer
            .query_row("PRAGMA main.wal_checkpoint(PASSIVE)", [], |row| {
                Ok((
                    row.get::<_, i32>(0)?,
                    row.get::<_, i32>(1)?,
                    row.get::<_, i32>(2)?,
                ))
            })
            .unwrap();
        // The reader connection is still open and the response is paused, but
        // its completed transaction cannot retain an old checkpoint end mark.
        let fully_checkpointed =
            checkpoint.0 == 0 && checkpoint.1 > 0 && checkpoint.1 == checkpoint.2;
        release_tx.send(()).unwrap();
        let snapshot = reader.join().unwrap().unwrap();
        *VIEW_SERIALIZE_HOOK.lock().unwrap() = None;
        assert!(fully_checkpointed, "{checkpoint:?}");
        assert_eq!(snapshot.scan.revision, prior_revision);
        assert!(snapshot
            .pages
            .iter()
            .all(|page| page.page.revision == prior_revision));
        assert_eq!(snapshot.chart.revision, prior_revision);
        assert_eq!(repo.scan(&scan_id).unwrap().revision, prior_revision + 1);
    }

    #[test]
    fn view_snapshot_stays_on_one_revision_while_writer_commits() {
        let (_fixture, repo, scan_id) = fixture();
        let prior_revision = repo.scan(&scan_id).unwrap().revision;
        let (entered_tx, entered_rx) = crossbeam_channel::bounded(1);
        let (release_tx, release_rx) = crossbeam_channel::bounded(1);
        *VIEW_READ_HOOK.lock().unwrap() = Some((repo.path.clone(), entered_tx, release_rx));
        struct ClearHook(crossbeam_channel::Sender<()>);
        impl Drop for ClearHook {
            fn drop(&mut self) {
                VIEW_READ_HOOK.lock().unwrap().take();
                let _ = self.0.try_send(());
            }
        }
        let clear = ClearHook(release_tx.clone());
        let reader_repo = repo.clone();
        let read_request = request(&scan_id);
        let reader = std::thread::spawn(move || reader_repo.view_snapshot(&read_request));
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut writer = repo.connect().unwrap();
        let tx = writer.transaction().unwrap();
        tx.execute(
            "UPDATE nodes SET logical_total=117 WHERE scan_id=?1",
            [&scan_id],
        )
        .unwrap();
        tx.execute(
            "UPDATE nodes SET payload=json_set(payload,'$.b',117) WHERE scan_id=?1 AND id=3",
            [&scan_id],
        )
        .unwrap();
        tx.execute(
            "UPDATE scans SET revision=revision+1 WHERE id=?1",
            [&scan_id],
        )
        .unwrap();
        tx.commit().unwrap();
        release_tx.send(()).unwrap();
        let snapshot = reader.join().unwrap().unwrap();
        drop(clear);
        assert_eq!(snapshot.scan.revision, prior_revision);
        assert_consistent(&snapshot, "17");
        let latest = repo.view_snapshot(&request(&scan_id)).unwrap();
        assert_eq!(latest.scan.revision, prior_revision + 1);
        assert_consistent(&latest, "117");
    }

    #[test]
    fn view_snapshot_keeps_collapsed_directories_and_page_anchor_instead_of_revealing_selection() {
        let (_fixture, repo, scan_id) = fixture();
        let mut c = repo.connect().unwrap();
        let tx = c.transaction().unwrap();
        let folder = repo.node(&scan_id, "2", None).unwrap().path;
        let metadata = platform::metadata(&folder.join("file.bin")).unwrap();
        for id in 4..=304 {
            insert_node(
                &tx,
                &scan_id,
                id,
                Some(2),
                &folder.join(format!("item-{id:04}.bin")),
                2,
                &metadata,
                None,
            )
            .unwrap();
        }
        tx.execute(
            "UPDATE scans SET revision=revision+1 WHERE id=?1",
            [&scan_id],
        )
        .unwrap();
        tx.commit().unwrap();
        let mut r = request(&scan_id);
        r.expanded_ids.clear();
        r.selected_ids = vec!["3".into(), "999999".into()];
        r.primary_id = Some("3".into());
        r.page_anchors = vec![ViewPageAnchor {
            parent_id: "2".into(),
            first_id: "103".into(),
        }];
        let snapshot = repo.view_snapshot(&r).unwrap();
        assert!(snapshot.expanded_ids.is_empty());
        assert_eq!(snapshot.selected_ids, ["3"]);
        let folder_page = &snapshot
            .pages
            .iter()
            .find(|page| page.parent_id == "2")
            .unwrap()
            .page;
        assert_eq!(folder_page.nodes[0].node_id, "103");
        assert!(folder_page.page_cursor.is_some());
        assert!(!folder_page.nodes.iter().any(|node| node.node_id == "3"));
        r.page_anchors[0].first_id = "1".into(); // Existing node in a different parent.
        assert_eq!(
            repo.view_snapshot(&r)
                .unwrap()
                .pages
                .iter()
                .find(|page| page.parent_id == "2")
                .unwrap()
                .page
                .nodes[0]
                .node_id,
            "3"
        );
        r.scope_id = "999999".into();
        let tx = c.transaction().unwrap();
        let root = repo.node(&scan_id, "1", None).unwrap().path;
        for id in 1000..1040 {
            let path = root.join(format!("expanded-{id}"));
            std::fs::create_dir(&path).unwrap();
            insert_node(
                &tx,
                &scan_id,
                id,
                Some(1),
                &path,
                1,
                &platform::metadata(&path).unwrap(),
                None,
            )
            .unwrap();
        }
        tx.commit().unwrap();
        r.expanded_ids = (1000..1040).map(|id| id.to_string()).collect();
        let fallback = repo.view_snapshot(&r).unwrap();
        assert_eq!(fallback.scope_id, "1");
        assert_eq!(fallback.expanded_ids, ["1"]);
    }

    #[test]
    fn view_snapshot_input_and_incremental_output_budgets_fail_closed() {
        let (_fixture, repo, scan_id) = fixture();
        let mut r = request(&scan_id);
        r.expanded_ids = vec!["1".into(); 41];
        assert_eq!(repo.view_snapshot(&r).unwrap_err().code, "VIEW_LIMIT");
        r.expanded_ids.clear();
        r.selected_ids = vec!["1".into(); 4097];
        assert_eq!(repo.view_snapshot(&r).unwrap_err().code, "SELECTION_LIMIT");
        r.selected_ids.clear();
        r.sort = Some("unknown".into());
        assert_eq!(repo.view_snapshot(&r).unwrap_err().code, "INVALID_SORT");
        let mut budget = ViewBudget { bytes: 0, nodes: 0 };
        assert_eq!(
            budget
                .charge(&"x".repeat(8 * 1024 * 1024), 0)
                .unwrap_err()
                .code,
            "VIEW_PAYLOAD_BUDGET"
        );
        let mut budget = ViewBudget { bytes: 0, nodes: 0 };
        assert_eq!(
            budget.charge(&"small", 10_001).unwrap_err().code,
            "VIEW_PAYLOAD_BUDGET"
        );
        // A budget stop on the first ancestor must occur before decoding a
        // later corrupt ancestor, rather than collecting the whole path first.
        let c = repo.connect().unwrap();
        c.execute(
            "UPDATE nodes SET payload='unreadable' WHERE scan_id=?1 AND id=1",
            [&scan_id],
        )
        .unwrap();
        let mut budget = ViewBudget {
            bytes: 8 * 1024 * 1024 - 1,
            nodes: 0,
        };
        assert_eq!(
            Repository::visit_ancestors_conn(&c, &scan_id, Some("2".into()), |node| budget
                .charge(&node, 1))
            .unwrap_err()
            .code,
            "VIEW_PAYLOAD_BUDGET"
        );
        assert!(Repository::node_conn(&c, &scan_id, 1).is_err());
    }
}

#[cfg(test)]
mod aggregate_many_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn fixture() -> (tempfile::TempDir, Repository, String, String) {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("source");
        let layout = [
            (1, None, "", 0),
            (20, Some(1), "A", 1),
            (2, Some(20), "A/nested", 2),
            (30, Some(1), "B", 1),
            (4, Some(30), "B/nested", 2),
            (6, Some(2), "A/nested/deep", 3),
            (7, Some(4), "B/nested/deep", 3),
        ];
        for (_, _, path, _) in &layout {
            std::fs::create_dir_all(root.join(path)).unwrap();
        }
        let root = platform::absolute(&root).unwrap();
        let repo = Repository::new(&fixture.path().join("db")).unwrap();
        let mut ids = Vec::new();
        for _ in 0..2 {
            let scan_id = repo.new_scan_id().unwrap();
            repo.insert_scan(&scan_id, &root, false).unwrap();
            let mut c = repo.connect().unwrap();
            let tx = c.transaction().unwrap();
            for (id, parent, path, depth) in &layout {
                let path = root.join(path);
                insert_node(
                    &tx,
                    &scan_id,
                    *id,
                    *parent,
                    &path,
                    *depth,
                    &platform::metadata(&path).unwrap(),
                    None,
                )
                .unwrap();
            }
            tx.execute("UPDATE nodes SET error_code='ACCESS_DENIED',incomplete=1 WHERE scan_id=?1 AND id=7", [&scan_id]).unwrap();
            tx.commit().unwrap();
            ids.push(scan_id);
        }
        (fixture, repo, ids.remove(0), ids.remove(0))
    }

    fn values(c: &Connection, scan_id: &str) -> serde_json::Value {
        let mut statement = c
            .prepare(&format!(
                "SELECT {NODE_COLUMNS} FROM nodes WHERE scan_id=?1 ORDER BY id"
            ))
            .unwrap();
        let nodes = statement
            .query_map([scan_id], read_node)
            .unwrap()
            .map(|row| row.unwrap().node)
            .collect::<Vec<_>>();
        let counters: (i64, i64) = c
            .query_row(
                "SELECT discovered,revision FROM scans WHERE id=?1",
                [scan_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        serde_json::json!({"nodes":nodes,"counters":counters})
    }

    fn apply(
        repo: &Repository,
        legacy: &str,
        folded: &str,
        changes: &BTreeMap<i64, [i64; 6]>,
        limit: usize,
    ) -> AggregateStats {
        let mut c = repo.connect().unwrap();
        let tx = c.transaction().unwrap();
        for (id, delta) in changes {
            aggregate_batch(
                &tx, legacy, *id, delta[0], delta[1], delta[2], delta[3], delta[4], delta[5],
            )
            .unwrap();
        }
        let stats = aggregate_many_with_limit(&tx, folded, changes, limit).unwrap();
        tx.execute(
            "UPDATE scans SET revision=revision+1 WHERE id IN (?1,?2)",
            params![legacy, folded],
        )
        .unwrap();
        assert_eq!(values(&tx, legacy), values(&tx, folded));
        tx.commit().unwrap();
        assert_eq!(values(&c, legacy), values(&c, folded));
        stats
    }

    #[test]
    fn coalesced_shared_ancestors_match_legacy_for_branches_nested_parents_and_signed_deltas() {
        let (_fixture, repo, legacy, folded) = fixture();
        let positive = BTreeMap::from([
            (20, [10, 8, 1, 2, 0, 2]),
            (30, [20, 15, 2, 3, 1, 4]),
            (2, [30, 20, 3, 4, 2, 6]),
            (6, [40, 30, 4, 5, 3, 8]),
        ]);
        let stats = apply(&repo, &legacy, &folded, &positive, 32_768);
        assert!(stats.coalesced);
        assert_eq!(stats.direct_parents, 4);
        assert_eq!(stats.ancestor_rows, 5); // Shared root/A parents are updated once.
        let c = repo.connect().unwrap();
        let root = Repository::node_conn(&c, &folded, 1).unwrap().node;
        assert_eq!(root.logical_total_bytes, "100");
        assert_eq!(root.children_count, "2"); // Never descendant count propagated upward.
        let discovered: i64 = c
            .query_row(
                "SELECT discovered FROM scans WHERE id=?1",
                [&folded],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(discovered, 27);
        assert_eq!(
            Repository::node_conn(&c, &folded, 20)
                .unwrap()
                .node
                .logical_total_bytes,
            "80"
        );
        let negative = BTreeMap::from([
            (20, [-2, -2, 0, 0, 0, -1]),
            (30, [-5, -3, -1, -1, 0, -1]),
            (2, [-5, -4, -1, -1, -1, -1]),
            (6, [-10, -5, -2, -2, -1, -2]),
        ]);
        assert!(apply(&repo, &legacy, &folded, &negative, 32_768).coalesced);
    }

    #[test]
    fn coalescing_limit_falls_back_before_any_update_and_matches_legacy() {
        let (_fixture, repo, legacy, folded) = fixture();
        let changes = BTreeMap::from([(2, [31, 20, 2, 2, 1, 3]), (7, [13, 9, 1, 1, 0, 1])]);
        let stats = apply(&repo, &legacy, &folded, &changes, 2);
        assert!(!stats.coalesced);
        assert_eq!(stats.direct_parents, 2);
        assert!(
            !apply(
                &repo,
                &legacy,
                &folded,
                &BTreeMap::from([(2, [-1, -1, 0, 0, 0, 0])]),
                0
            )
            .coalesced
        );
    }

    #[test]
    fn coalesced_overflow_rejects_before_updates_and_rolls_back_batch_revision() {
        let (_fixture, repo, _legacy, folded) = fixture();
        let mut c = repo.connect().unwrap();
        let before = values(&c, &folded);
        let tx = c.transaction().unwrap();
        let changes = BTreeMap::from([(2, [i64::MAX, 0, 0, 0, 0, 0]), (7, [1, 0, 0, 0, 0, 0])]);
        assert_eq!(
            aggregate_many(&tx, &folded, &changes).unwrap_err().code,
            "SIZE_OUT_OF_RANGE"
        );
        assert_eq!(values(&tx, &folded), before);
        // Other work in this uncommitted batch is not exposed after an error.
        tx.execute(
            "UPDATE scans SET revision=revision+1 WHERE id=?1",
            [&folded],
        )
        .unwrap();
        tx.rollback().unwrap();
        assert_eq!(values(&c, &folded), before);
        let tx = c.transaction().unwrap();
        let underflow = BTreeMap::from([(2, [-1, 0, 0, 0, 0, 0])]);
        assert_eq!(
            aggregate_many(&tx, &folded, &underflow).unwrap_err().code,
            "SIZE_OUT_OF_RANGE"
        );
        assert_eq!(values(&tx, &folded), before);
    }

    #[test]
    fn fallback_overflow_after_prior_parent_updates_rolls_back_every_counter_and_revision() {
        let (_fixture, repo, _legacy, folded) = fixture();
        let mut c = repo.connect().unwrap();
        let before = values(&c, &folded);
        let tx = c.transaction().unwrap();
        tx.execute(
            "UPDATE scans SET revision=revision+1 WHERE id=?1",
            [&folded],
        )
        .unwrap();
        let changes = BTreeMap::from([(2, [i64::MAX, 0, 0, 0, 0, 0]), (7, [1, 0, 0, 0, 0, 0])]);
        assert_eq!(
            aggregate_many_with_limit(&tx, &folded, &changes, 0)
                .unwrap_err()
                .code,
            "SIZE_OUT_OF_RANGE"
        );
        // The legacy fallback has already changed A/shared root when B
        // overflows. No observer may see those uncommitted partial changes.
        assert_ne!(values(&tx, &folded), before);
        assert_eq!(values(&repo.connect().unwrap(), &folded), before);
        tx.rollback().unwrap();
        assert_eq!(values(&c, &folded), before);
    }
}
