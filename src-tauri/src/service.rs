use crate::{model::*, platform, repository::Repository};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

pub type ProgressCallback = Arc<dyn Fn(JobEvent) + Send + Sync>;
pub struct JobRuntime {
    pub cancel: Arc<AtomicBool>,
    pub event: Mutex<JobEvent>,
    pub started: Instant,
    pub callback: Option<ProgressCallback>,
    pub last_sent: AtomicU64,
    pub acknowledged: AtomicU64,
    pub bounded_progress: AtomicBool,
}
impl JobRuntime {
    pub fn new(event: JobEvent, callback: Option<ProgressCallback>) -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            event: Mutex::new(event),
            started: Instant::now(),
            callback,
            last_sent: AtomicU64::new(0),
            acknowledged: AtomicU64::new(0),
            bounded_progress: AtomicBool::new(true),
        }
    }
    pub fn update(&self, update: impl FnOnce(&mut JobEvent)) {
        let e = {
            let mut event = self.event.lock().unwrap();
            update(&mut event);
            event.seq += 1;
            event.elapsed_ms = self.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
            event.clone()
        };
        self.send_bounded(e);
    }
    pub fn snapshot(&self) -> JobEvent {
        self.event.lock().unwrap().clone()
    }
    pub fn terminal(&self) -> bool {
        matches!(
            self.event.lock().unwrap().status.as_str(),
            "completed" | "completed_with_errors" | "cancelled" | "failed" | "interrupted"
        )
    }
    fn send_bounded(&self, event: JobEvent) {
        if let Some(callback) = &self.callback {
            let sent = self.last_sent.load(Ordering::Acquire);
            let terminal = matches!(
                event.status.as_str(),
                "completed" | "completed_with_errors" | "cancelled" | "failed" | "interrupted"
            );
            if (!self.bounded_progress.load(Ordering::Acquire)
                || terminal
                || sent == self.acknowledged.load(Ordering::Acquire))
                && event.seq > sent
                && self
                    .last_sent
                    .compare_exchange(sent, event.seq, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                callback(event);
            }
        }
    }
    pub fn acknowledge(&self, seq: u64) -> Result<()> {
        let sent = self.last_sent.load(Ordering::Acquire);
        if seq > sent {
            return Err(AppError::new("INVALID_ACK", "进度确认序号无效"));
        }
        self.acknowledged.fetch_max(seq, Ordering::AcqRel);
        self.send_bounded(self.snapshot());
        Ok(())
    }
}
pub struct Service {
    pub repo: Repository,
    pub writer: Mutex<()>,
    pub sources: Mutex<HashMap<String, PathBuf>>,
    pub jobs: Mutex<HashMap<String, Arc<JobRuntime>>>,
    pub plans: Mutex<HashMap<String, crate::file_ops::StoredPlan>>,
    pub work_files: Mutex<HashSet<PathBuf>>,
    pub settings_lock: Mutex<()>,
}
impl Service {
    /// Supplement selected-item metadata without doing a handle query for every scanned file.
    pub fn measure_node(&self, scan_id: &str, node_id: &str, revision: u32) -> Result<(Node, u32)> {
        let stored = self.repo.node(scan_id, node_id, Some(revision))?;
        if stored.node.allocated_bytes.is_some() && stored.node.identity.is_some() {
            return Ok((stored.node, revision));
        }
        let meta = platform::metadata(&stored.path)?;
        if meta.kind != stored.fingerprint.kind
            || meta.logical != stored.fingerprint.logical
            || meta.modified != stored.fingerprint.modified
            || meta.reparse_tag != stored.fingerprint.reparse_tag
            || stored
                .fingerprint
                .identity
                .as_ref()
                .is_some_and(|id| meta.identity.as_ref() != Some(id))
        {
            return Err(AppError::new(
                "SOURCE_CHANGED",
                "扫描后项目已变化，请刷新后测量",
            ));
        }
        let _writer = self.writer.lock().unwrap();
        let mut c = self.repo.connect()?;
        let tx = c.transaction()?;
        Repository::check_revision(&tx, scan_id, revision)?;
        let old_alloc = stored
            .node
            .allocated_bytes
            .as_deref()
            .map(parse_bytes)
            .transpose()?
            .unwrap_or(0);
        let new_alloc = meta.allocated.map(integer).transpose()?.unwrap_or(0);
        let unknown_delta =
            i64::from(meta.allocated.is_none()) - i64::from(stored.node.allocated_bytes.is_none());
        let mut node = stored.node;
        node.identity = meta.identity.clone();
        node.link_count = meta.links.map(|n| n.to_string());
        node.link_target = meta.link_target.clone();
        node.allocation_method = meta.allocation_method.clone();
        node.allocated_bytes = meta.allocated.map(|n| n.to_string());
        tx.execute("UPDATE nodes SET payload=?3,allocated_total=allocated_total+?4,allocated_unknown=allocated_unknown+?5 WHERE scan_id=?1 AND id=?2",rusqlite::params![scan_id,parse_id(node_id)?,serde_json::to_string(&serde_json::json!({"node":node,"fingerprint":meta.fingerprint()}))?,new_alloc-old_alloc,unknown_delta])?;
        if let Some(parent) = node.parent_id.as_deref() {
            crate::repository::aggregate_batch(
                &tx,
                scan_id,
                parse_id(parent)?,
                0,
                new_alloc - old_alloc,
                unknown_delta,
                0,
                0,
                0,
            )?;
        }
        tx.execute(
            "UPDATE scans SET revision=revision+1 WHERE id=?1",
            [scan_id],
        )?;
        let node = Repository::node_conn(&tx, scan_id, parse_id(node_id)?)?.node;
        let new_revision = Repository::scan_conn(&tx, scan_id)?.revision;
        tx.commit()?;
        Ok((node, new_revision))
    }
    pub fn new(data_dir: &Path) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            repo: Repository::new(data_dir)?,
            writer: Mutex::new(()),
            sources: Mutex::new(HashMap::new()),
            jobs: Mutex::new(HashMap::new()),
            plans: Mutex::new(HashMap::new()),
            work_files: Mutex::new(HashSet::new()),
            settings_lock: Mutex::new(()),
        }))
    }
    pub fn register_source(&self, path: PathBuf) -> Result<Source> {
        let p = Self::resolve_directory(&path)?;
        if !p.is_dir() {
            return Err(AppError::new("NOT_DIRECTORY", "请选择目录"));
        }
        let source = Source {
            source_ref: uuid::Uuid::new_v4().to_string(),
            display_path: platform::display(&p),
        };
        let mut map = self.sources.lock().unwrap();
        if map.len() >= 64 {
            map.clear()
        }
        map.insert(source.source_ref.clone(), p);
        Ok(source)
    }
    pub fn resolve_source(&self, source_ref: Option<&str>, path: Option<&str>) -> Result<PathBuf> {
        if let Some(id) = source_ref {
            self.sources
                .lock()
                .unwrap()
                .get(id)
                .cloned()
                .ok_or_else(|| AppError::new("SOURCE_EXPIRED", "位置引用已失效，请重新选择"))
        } else if let Some(path) = path {
            if path.len() > 131072 {
                return Err(AppError::new("PATH_LIMIT", "路径过长"));
            }
            Self::resolve_directory(Path::new(path))
        } else {
            Err(AppError::new("INVALID_SOURCE", "请选择扫描位置"))
        }
    }
    fn resolve_directory(path: &Path) -> Result<PathBuf> {
        if !path.is_absolute() {
            return Err(AppError::new("INVALID_PATH", "请输入绝对路径"));
        }
        let metadata = platform::metadata(path)?;
        if metadata.kind != "directory" || !metadata.recurse || metadata.link_type.is_some() {
            return Err(AppError::new(
                "UNSAFE_SCAN_ROOT",
                "扫描/目标位置必须为可本地枚举的实际目录；根跳转链接和需召回目录未跟随",
            ));
        }
        platform::absolute(path)
    }
    pub fn is_work_file(&self, path: &Path) -> bool {
        self.work_files.lock().unwrap().contains(path)
    }
    pub fn add_job(
        &self,
        id: &str,
        event: JobEvent,
        callback: Option<ProgressCallback>,
    ) -> Result<Arc<JobRuntime>> {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs
            .values()
            .any(|j| !j.terminal() && j.snapshot().r#type == event.r#type)
        {
            return Err(AppError::new(
                "JOB_BUSY",
                "同类任务正在运行，请等待完成或停止",
            ));
        }
        if jobs.len() >= 64 {
            jobs.retain(|_, j| !j.terminal());
        }
        let job = Arc::new(JobRuntime::new(event, callback));
        jobs.insert(id.into(), job.clone());
        Ok(job)
    }
    pub fn job(&self, id: &str) -> Result<Arc<JobRuntime>> {
        self.jobs
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::new("JOB_NOT_FOUND", "任务记录已过期"))
    }
    pub fn cancel_job(&self, id: &str) -> Result<()> {
        let job = self.job(id)?;
        if job.terminal() {
            return Ok(());
        }
        job.cancel.store(true, Ordering::Release);
        job.update(|e| {
            e.status = "cancelling".into();
            e.phase = "正在结束当前读取，保留已完成部分".into()
        });
        Ok(())
    }
    pub fn start_scan(
        self: &Arc<Self>,
        path: PathBuf,
        callback: Option<ProgressCallback>,
    ) -> Result<Scan> {
        let meta = platform::metadata(&path)?;
        if meta.kind != "directory" || !meta.recurse {
            return Err(AppError::new(
                "UNSAFE_SCAN_ROOT",
                "扫描根必须是可枚举的本地目录；链接或需召回云目录未跟随",
            ));
        }
        let id = self.repo.new_scan_id()?;
        let job = self.add_job(&id, JobEvent::new(&id, &id, "scan", 1), callback)?;
        {
            let _writer = self.writer.lock().unwrap();
            self.repo.insert_scan(&id, &path, false)?;
            let mut c = self.repo.connect()?;
            let tx = c.transaction()?;
            crate::repository::insert_node(&tx, &id, 1, None, &path, 0, &meta, None)?;
            tx.commit()?;
        }
        let scan = self.repo.scan(&id)?;
        let service = self.clone();
        let worker_id = id.clone();
        std::thread::spawn(move || {
            if let Err(error) = crate::scan::scan_sync(&service, &worker_id, &path, &job) {
                let result = {
                    let _writer = service.writer.lock().unwrap();
                    service.repo.finish(&worker_id, "failed")
                };
                job.update(|e| {
                    e.status = "failed".into();
                    e.phase = "扫描失败，已提交结果仍可查看".into();
                    e.error = Some(error);
                    if let Ok(scan) = result {
                        e.revision = scan.revision;
                        e.discovered_count = scan.discovered_count;
                        e.logical_total_bytes = scan.logical_total_bytes;
                        e.error_count = scan.error_count;
                    }
                })
            }
        });
        let mut settings = self.read_settings();
        settings.recent_locations.retain(|p| p != &scan.source);
        settings.recent_locations.insert(0, scan.source.clone());
        settings.recent_locations.truncate(10);
        let _ = self.save_settings(settings);
        Ok(scan)
    }
    pub fn read_settings(&self) -> Settings {
        let _lock = self.settings_lock.lock().unwrap();
        std::fs::read(self.repo.data_dir.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Settings>(&b).ok())
            .filter(|s| s.validate().is_ok())
            .unwrap_or_default()
    }
    pub fn save_settings(&self, settings: Settings) -> Result<Settings> {
        settings.validate()?;
        let _lock = self.settings_lock.lock().unwrap();
        let mut temp = tempfile::NamedTempFile::new_in(&self.repo.data_dir)?;
        use std::io::Write;
        temp.write_all(&serde_json::to_vec_pretty(&settings)?)?;
        temp.as_file().sync_all()?;
        temp.persist(self.repo.data_dir.join("settings.json"))
            .map_err(|e| AppError::from(e.error))?;
        Ok(settings)
    }
    pub fn refresh(
        self: &Arc<Self>,
        scan_id: String,
        scope_id: String,
        callback: Option<ProgressCallback>,
    ) -> Result<Scan> {
        let original = self.repo.scan(&scan_id)?;
        let target = self
            .repo
            .node(&scan_id, &scope_id, Some(original.revision))?;
        if target.node.kind != "directory" {
            return Err(AppError::new("NOT_DIRECTORY", "仅能刷新目录"));
        }
        let job = self.add_job(
            &scan_id,
            JobEvent::new(&scan_id, &scan_id, "scan", original.revision),
            callback,
        )?;
        let staged = self.repo.new_scan_id()?;
        {
            let _writer = self.writer.lock().unwrap();
            self.repo.insert_scan(&staged, &target.path, true)?;
        }
        let old_revision = original.revision;
        let service = self.clone();
        std::thread::spawn(move || {
            // Staging progress does not invalidate the visible target until atomic replacement.
            job.update(|e| {
                e.status = "running".into();
                e.phase = "准备局部扫描".into()
            });
            let forwarding_job = job.clone();
            let mut inner = JobRuntime::new(
                JobEvent::new(&staged, &staged, "scan", 1),
                Some(Arc::new(move |event| {
                    forwarding_job.update(|e| {
                        e.phase = format!("刷新目录：{}", event.phase);
                        e.revision = old_revision;
                        e.discovered_count = event.discovered_count;
                        e.logical_total_bytes = event.logical_total_bytes;
                        e.error_count = event.error_count;
                    });
                })),
            );
            inner.cancel = job.cancel.clone();
            inner.bounded_progress.store(false, Ordering::Release);
            let result = crate::scan::scan_sync(&service, &staged, &target.path, &inner);
            match result {
                Ok(s) if matches!(s.status.as_str(), "completed" | "completed_with_errors") => {
                    match crate::scan::replace_subtree(&service, &scan_id, &scope_id, &staged) {
                        Ok(revision) => {
                            let s = service.repo.scan(&scan_id).ok();
                            job.update(|e| {
                                e.revision = revision;
                                e.status = "completed".into();
                                e.phase = "目录已刷新".into();
                                if let Some(s) = s {
                                    e.logical_total_bytes = s.logical_total_bytes;
                                    e.discovered_count = s.discovered_count;
                                    e.error_count = s.error_count;
                                }
                            })
                        }
                        Err(error) => job.update(|e| {
                            e.status = "failed".into();
                            e.error = Some(error)
                        }),
                    }
                }
                Ok(_) => job.update(|e| {
                    e.status = "cancelled".into();
                    e.phase = "刷新已取消，保留原目录结果".into()
                }),
                Err(error) => job.update(|e| {
                    e.status = "failed".into();
                    e.error = Some(error)
                }),
            }
            let _writer = service.writer.lock().unwrap();
            if let Ok(c) = service.repo.connect() {
                let _ = c.execute("DELETE FROM scans WHERE id=?1", [&staged]);
                let _ = c.execute("DELETE FROM scan_errors WHERE scan_id=?1", [&staged]);
            }
        });
        let mut returned = original;
        returned.status = "running".into();
        Ok(returned)
    }
}
