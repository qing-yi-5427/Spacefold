use crate::{
    model::*,
    platform,
    repository::Repository,
    service::{JobRuntime, ProgressCallback, Service},
};
use std::{
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct PlannedSource {
    pub stored: StoredNode,
    pub fingerprint: Fingerprint,
    pub parents: Vec<(PathBuf, Fingerprint)>,
    pub destination: Option<PathBuf>,
    pub destination_existing: Option<Fingerprint>,
    pub destination_parent: Option<(PathBuf, Fingerprint)>,
    pub destination_parents: Vec<(PathBuf, Fingerprint)>,
    pub error: Option<AppError>,
}
#[derive(Clone)]
pub struct StoredPlan {
    pub dto: OperationPlan,
    pub sources: Vec<PlannedSource>,
    pub policy: String,
    pub created: Instant,
    pub estimated_bytes: usize,
}
fn fingerprint_matches(a: &Fingerprint, b: &Fingerprint) -> bool {
    (a.identity.is_none() || a.identity == b.identity)
        && b.identity.is_some()
        && a.kind == b.kind
        && a.logical == b.logical
        && a.modified == b.modified
        && a.reparse_tag == b.reparse_tag
}
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        || name.ends_with([' ', '.'])
        || name.encode_utf16().count() > 255
    {
        return Err(AppError::new(
            "INVALID_NAME",
            "名称无效：不能包含路径分隔符、控制字符或 Windows 保留字符",
        ));
    }
    let base = name.split('.').next().unwrap_or("").to_uppercase();
    if [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ]
    .contains(&base.as_str())
    {
        return Err(AppError::new(
            "INVALID_NAME",
            "Windows 保留名称不能作为文件名",
        ));
    }
    Ok(())
}
fn keep_both(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::new("INVALID_DESTINATION", "目标必须有父目录"))?;
    let stem = path.file_stem().unwrap_or_default();
    let ext = path.extension();
    for i in 2..10000 {
        let mut name = stem.to_os_string();
        name.push(format!(" ({i})"));
        if let Some(ext) = ext {
            name.push(".");
            name.push(ext);
        }
        let p = parent.join(name);
        if !p.try_exists()? {
            return Ok(p);
        }
    }
    Err(AppError::new(
        "DESTINATION_CONFLICT",
        "无法生成未使用的新名称",
    ))
}
pub fn plan(service: &Service, request: OperationRequest) -> Result<OperationPlan> {
    // Validate typed text before canonicalize can silently strip a Win32 suffix.
    let typed_destination_error = request
        .destination_path
        .as_deref()
        .map(Path::new)
        .map(platform::validate_shell_path)
        .transpose()
        .err();
    if !["copy", "move", "rename", "trash"].contains(&request.kind.as_str()) {
        return Err(AppError::new("INVALID_OPERATION", "文件操作类型无效"));
    }
    if request.node_ids.is_empty() || request.node_ids.len() > 512 {
        return Err(AppError::new("SELECTION_LIMIT", "请选择 1–512 个项目"));
    }
    let policy = request
        .conflict_policy
        .clone()
        .unwrap_or_else(|| "skip".into());
    if !["skip", "keep_both", "replace"].contains(&policy.as_str()) {
        return Err(AppError::new("INVALID_CONFLICT_POLICY", "冲突策略无效"));
    }
    let c = service.repo.connect()?;
    let tx = c.unchecked_transaction()?;
    Repository::check_revision(&tx, &request.scan_id, request.expected_revision)?;
    let scan = Repository::scan_conn(&tx, &request.scan_id)?;
    if matches!(scan.status.as_str(), "queued" | "running" | "cancelling") {
        return Err(AppError::new(
            "SCAN_BUSY",
            "请停止当前扫描或等待完成后管理文件",
        ));
    }
    let (nodes, _) = Repository::normalize_conn(&tx, &request.scan_id, &request.node_ids)?;
    if request.kind == "rename" && nodes.len() != 1 {
        return Err(AppError::new(
            "SINGLE_ITEM_REQUIRED",
            "重命名只能选择一个项目",
        ));
    }
    let destination = if matches!(request.kind.as_str(), "copy" | "move") {
        Some(service.resolve_source(
            request.destination_ref.as_deref(),
            request.destination_path.as_deref(),
        )?)
    } else {
        None
    };
    if let Some(name) = request.new_name.as_deref() {
        validate_name(name)?
    }
    if request.kind == "rename" && request.new_name.is_none() {
        return Err(AppError::new("INVALID_NAME", "请输入新名称"));
    }
    let mut sources = vec![];
    let mut items = vec![];
    let mut total = 0i64;
    let mut destinations = std::collections::HashSet::new();
    for node in nodes {
        let mut source = PlannedSource {
            stored: node.clone(),
            fingerprint: node.fingerprint.clone(),
            parents: vec![],
            destination: None,
            destination_existing: None,
            destination_parent: None,
            destination_parents: vec![],
            error: None,
        };
        let validation = (|| -> Result<()> {
            platform::validate_shell_path(&node.path)?;
            if node.node.parent_id.is_none() && request.kind != "copy" {
                return Err(AppError::new(
                    "SCAN_ROOT_PROTECTED",
                    "扫描根不能执行文件管理操作",
                ));
            }
            let metadata = platform::metadata(&node.path)?;
            let current = metadata.fingerprint();
            if !fingerprint_matches(&node.fingerprint, &current) {
                return Err(AppError::new(
                    "SOURCE_CHANGED",
                    "扫描后项目已变化，请先刷新",
                ));
            }
            if metadata.identity.is_none() {
                return Err(AppError::new(
                    "IDENTITY_UNAVAILABLE",
                    "无法可靠核验来源身份",
                ));
            }
            source.fingerprint = current;
            source.parents = platform::parent_chain(&node.path)?;
            if matches!(request.kind.as_str(), "copy" | "move" | "trash") {
                validate_source_tree(&node.path, &metadata)?;
            }
            if matches!(request.kind.as_str(), "copy" | "move") {
                if let Some(error) = &typed_destination_error {
                    return Err(error.clone());
                }
                let dest = destination.as_ref().unwrap();
                platform::validate_shell_path(dest)?;
                if dest.starts_with(&service.repo.data_dir) {
                    return Err(AppError::new(
                        "APP_DATA_PROTECTED",
                        "不能把文件写入本应用的索引/设置目录",
                    ));
                }
                let dm = platform::metadata(dest)?;
                if dm.kind != "directory" || !dm.recurse {
                    return Err(AppError::new(
                        "UNSAFE_DESTINATION",
                        "目标必须是本地实际目录，不能是链接或需召回的云目录",
                    ));
                }
                if dest == &node.path || dest.starts_with(&node.path) {
                    return Err(AppError::new(
                        "DESTINATION_IS_DESCENDANT",
                        "目标不能是来源自身或其后代",
                    ));
                }
                source.destination = Some(
                    dest.join(
                        node.path
                            .file_name()
                            .ok_or_else(|| AppError::new("INVALID_SOURCE", "来源名称无效"))?,
                    ),
                );
                source.destination_parent = Some((dest.clone(), dm.fingerprint()));
                source.destination_parents =
                    platform::parent_chain(&dest.join("__spacefold_validation__"))?;
                if dm.identity == source.fingerprint.identity
                    || source
                        .destination_parents
                        .iter()
                        .any(|(_, p)| p.identity == source.fingerprint.identity)
                {
                    return Err(AppError::new(
                        "DESTINATION_IS_DESCENDANT",
                        "目标目录的文件系统身份位于来源目录内",
                    ));
                }
            } else if request.kind == "rename" {
                let parent = node
                    .path
                    .parent()
                    .ok_or_else(|| AppError::new("SCAN_ROOT_PROTECTED", "根目录不能重命名"))?;
                source.destination = Some(parent.join(request.new_name.as_ref().unwrap()));
                source.destination_parent =
                    Some((parent.to_owned(), platform::metadata(parent)?.fingerprint()));
                source.destination_parents = platform::parent_chain(&node.path)?;
            } else {
                recycle_preflight(&node.path)?;
            }
            if let Some(dest) = source.destination.clone() {
                platform::validate_shell_path(&dest)?;
                if let Ok(m) = platform::metadata(&dest) {
                    let same = m.identity == source.fingerprint.identity;
                    let case_rename = request.kind == "rename" && same && dest != node.path;
                    if same && !case_rename {
                        return Err(AppError::new(
                            "DESTINATION_IS_SOURCE",
                            "目标与来源是同一个文件",
                        ));
                    }
                    if !case_rename {
                        if policy == "keep_both" {
                            source.destination = Some(keep_both(&dest)?)
                        } else {
                            if policy == "replace" {
                                validate_source_tree(&dest, &m)?;
                            }
                            source.destination_existing = Some(m.fingerprint());
                        }
                    }
                } else if dest.try_exists()? {
                    return Err(AppError::new(
                        "DESTINATION_UNREADABLE",
                        "目标存在但无法核验身份",
                    ));
                }
                let final_dest = source.destination.as_ref().unwrap();
                if !destinations.insert(platform::encode_path(final_dest)) {
                    return Err(AppError::new(
                        "DESTINATION_CONFLICT",
                        "多个来源将写入相同目标",
                    ));
                }
            }
            Ok(())
        })();
        source.error = validation.err();
        total =
            crate::repository::checked_add(total, parse_bytes(&node.node.logical_total_bytes)?)?;
        items.push(PlanItem {
            node_id: node.node.node_id.clone(),
            name: node.node.name.clone(),
            display_path: node.node.display_path.clone(),
            destination: source.destination.as_ref().map(|p| platform::display(p)),
            error: source.error.clone(),
        });
        sources.push(source);
    }
    let dto = OperationPlan {
        plan_id: uuid::Uuid::new_v4().to_string(),
        scan_id: request.scan_id,
        revision: request.expected_revision,
        kind: request.kind,
        items,
        logical_total_bytes: total.to_string(),
        expires_at: (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
        warnings: vec![
            "执行前会再次核验身份；跨卷操作可能部分完成，取消只停止尚未完成的项目。".into(),
        ],
    };
    let estimated_bytes = sources
        .iter()
        .map(|s| {
            platform::encode_path(&s.stored.path).len()
                + s.parents
                    .iter()
                    .chain(s.destination_parents.iter())
                    .map(|(p, _)| platform::encode_path(p).len() + 256)
                    .sum::<usize>()
        })
        .sum::<usize>()
        + serde_json::to_vec(&dto)?.len();
    if serde_json::to_vec(&dto)?.len() > 2 * 1024 * 1024 {
        return Err(AppError::new(
            "IPC_PAYLOAD_LIMIT",
            "操作计划超过 2 MiB 显示预算，请减少单次选择",
        ));
    }
    let mut plans = service.plans.lock().unwrap();
    plans.retain(|_, p| p.created.elapsed() < Duration::from_secs(300));
    if plans.len() >= 64 {
        return Err(AppError::new(
            "PLAN_LIMIT",
            "待执行计划过多，请关闭旧对话框后重试",
        ));
    }
    if estimated_bytes + plans.values().map(|p| p.estimated_bytes).sum::<usize>() > 32 * 1024 * 1024
    {
        return Err(AppError::new(
            "PLAN_MEMORY_BUDGET",
            "操作计划超过 32 MiB 内存预算，请减少选择或关闭旧计划",
        ));
    }
    plans.insert(
        dto.plan_id.clone(),
        StoredPlan {
            dto: dto.clone(),
            sources,
            policy,
            created: Instant::now(),
            estimated_bytes,
        },
    );
    Ok(dto)
}
fn validate_execution(source: &PlannedSource, kind: &str) -> Result<()> {
    platform::validate_shell_path(&source.stored.path)?;
    if let Some(dest) = &source.destination {
        platform::validate_shell_path(dest)?;
    }
    if let Some(error) = &source.error {
        return Err(error.clone());
    }
    let current = platform::metadata(&source.stored.path)?.fingerprint();
    if !fingerprint_matches(&source.fingerprint, &current) {
        return Err(AppError::new(
            "SOURCE_CHANGED",
            "计划生成后来源已变化，已停止此项",
        ));
    }
    if matches!(kind, "copy" | "move" | "trash") {
        let meta = platform::metadata(&source.stored.path)?;
        validate_source_tree(&source.stored.path, &meta)?;
    }
    platform::verify_parents(&source.parents)?;
    platform::verify_parents(&source.destination_parents)?;
    if let Some((p, fp)) = &source.destination_parent {
        let m = platform::metadata(p)?;
        if m.identity != fp.identity || m.kind != "directory" || m.reparse_tag != fp.reparse_tag {
            return Err(AppError::new("SOURCE_CHANGED", "目标目录已变化"));
        }
    }
    if let Some(dest) = &source.destination {
        if source.destination_existing.is_some() {
            let metadata = platform::metadata(dest)?;
            validate_source_tree(dest, &metadata)?;
        }
        let current = match platform::metadata(dest) {
            Ok(m) => Some(m.fingerprint()),
            Err(e) if e.code == "NOT_FOUND" => None,
            Err(e) => return Err(e),
        };
        let case_only = kind == "rename"
            && current
                .as_ref()
                .is_some_and(|m| m.identity == source.fingerprint.identity)
            && dest != &source.stored.path;
        if !case_only {
            match (&source.destination_existing, current) {
                (None, None) => {}
                (Some(a), Some(b)) if fingerprint_matches(a, &b) => {}
                _ => {
                    return Err(AppError::new(
                        "DESTINATION_CONFLICT",
                        "计划生成后目标发生冲突或被替换",
                    ))
                }
            }
        }
    }
    if kind == "trash" {
        recycle_preflight(&source.stored.path)?;
    }
    Ok(())
}
fn validate_source_tree(path: &Path, meta: &platform::Metadata) -> Result<()> {
    platform::validate_shell_path(path)?;
    if meta.link_type.is_some() {
        return Err(AppError::new(
            "UNSUPPORTED_LINK_OPERATION",
            "链接的复制/移动/回收无法确认不会跟随目标，已保守拒绝",
        ));
    }
    if meta.kind != "directory" {
        return Ok(());
    }
    if !meta.recurse {
        return Err(AppError::new(
            "UNSAFE_SOURCE_TREE",
            "此目录无法安全枚举以复核操作范围",
        ));
    }
    let mut stack = vec![platform::read_dir(path)?];
    while let Some(iter) = stack.last_mut() {
        match iter.next() {
            None => {
                stack.pop();
            }
            Some(Err(e)) => return Err(e),
            Some(Ok(entry)) => {
                platform::validate_shell_path(&entry.path)?;
                let m = entry
                    .hint
                    .map(Ok)
                    .unwrap_or_else(|| platform::metadata(&entry.path))?;
                if m.link_type.is_some() || m.kind == "directory" && !m.recurse {
                    return Err(AppError::new(
                        "UNSAFE_SOURCE_TREE",
                        "目录含跳转链接或不能本地枚举的云目录；已拒绝可能越界的 Shell 操作",
                    ));
                }
                if m.recurse {
                    let current = platform::metadata(&entry.path)?;
                    if current.kind != "directory" || !current.recurse {
                        return Err(AppError::new(
                            "SOURCE_CHANGED",
                            "目录在操作复核期间变为跳转链接或不可枚举",
                        ));
                    }
                    if stack.len() >= 4096 {
                        return Err(AppError::new("DEPTH_LIMIT", "源目录深度超过操作复核预算"));
                    }
                    stack.push(platform::read_dir(&entry.path)?);
                }
            }
        }
    }
    Ok(())
}
pub fn execute(
    service: &Arc<Service>,
    plan_id: &str,
    callback: Option<ProgressCallback>,
) -> Result<Job> {
    let plan = service
        .plans
        .lock()
        .unwrap()
        .remove(plan_id)
        .ok_or_else(|| AppError::new("PLAN_NOT_FOUND", "操作计划已执行或已过期"))?;
    if plan.created.elapsed() > Duration::from_secs(300) {
        return Err(AppError::new("PLAN_EXPIRED", "计划已过期，请重新预览"));
    }
    let c = service.repo.connect()?;
    Repository::check_revision(&c, &plan.dto.scan_id, plan.dto.revision)?;
    let id = uuid::Uuid::new_v4().to_string();
    let job = service.add_job(
        &id,
        JobEvent::new(&id, &plan.dto.scan_id, "operation", plan.dto.revision),
        callback,
    )?;
    let service = service.clone();
    let scan_id = plan.dto.scan_id.clone();
    let result = Job {
        job_id: id.clone(),
        scan_id,
    };
    std::thread::spawn(move || {
        execute_sync(&service, &plan, &id, &job);
    });
    Ok(result)
}
pub fn execute_sync(
    service: &Service,
    plan: &StoredPlan,
    job_id: &str,
    job: &JobRuntime,
) -> Vec<OperationResult> {
    let mut results = vec![];
    let mut attempted = std::collections::HashSet::new();
    job.update(|e| {
        e.status = "running".into();
        e.phase = "核验并执行文件操作".into()
    });
    for source in &plan.sources {
        let mut result = OperationResult {
            node_id: source.stored.node.node_id.clone(),
            display_path: source.stored.node.display_path.clone(),
            status: "failed".into(),
            destination: source.destination.as_ref().map(|p| platform::display(p)),
            error: None,
        };
        if job.cancel.load(Ordering::Acquire) {
            result.status = "cancelled".into();
            result.error = Some(AppError::new("CANCELLED", "已取消尚未开始的项目"));
        } else {
            match validate_execution(source, &plan.dto.kind) {
                Err(e) => result.error = Some(e),
                Ok(()) => {
                    if plan.policy == "skip" && source.destination_existing.is_some() {
                        result.status = "skipped".into();
                        result.error = Some(AppError::new(
                            "DESTINATION_CONFLICT",
                            "目标同名项目已存在，按策略跳过",
                        ));
                    } else {
                        attempted.insert(source.stored.node.node_id.clone());
                        match system_operation(source, &plan.dto.kind, &plan.policy, job) {
                            Ok(actual) => {
                                result.status = "success".into();
                                if let Some(p) = actual {
                                    result.destination = Some(platform::display(&p));
                                }
                            }
                            Err(e) => {
                                result.status = if e.code == "CANCELLED" {
                                    "cancelled"
                                } else {
                                    "failed"
                                }
                                .into();
                                result.error = Some(e);
                            }
                        }
                    }
                }
            }
        }
        results.push(result);
        let completed = results.len();
        let errors = results.iter().filter(|r| r.status == "failed").count();
        job.update(|e| {
            e.written_count = completed.to_string();
            e.error_count = errors.to_string();
            e.results = Some(results.clone());
        });
    }
    let any = !attempted.is_empty();
    let mut revision = plan.dto.revision;
    if any {
        match refresh_affected(service, plan, &attempted, job) {
            Ok(r) => revision = r,
            Err(error) => {
                revision = service
                    .repo
                    .scan(&plan.dto.scan_id)
                    .map(|s| s.revision)
                    .unwrap_or(revision);
                job.update(|e| {
                    e.error = Some(AppError::new(
                        "REFRESH_FAILED",
                        format!("文件操作已完成，但目录刷新失败：{}", error.message),
                    ))
                });
            }
        }
    }
    if let Err(error) =
        service
            .repo
            .log_operation(job_id, &plan.dto.scan_id, &plan.dto.kind, &results)
    {
        job.update(|e| e.error = Some(error));
    }
    job.update(|e| {
        e.revision = revision;
        e.status = if job.cancel.load(Ordering::Acquire) {
            "cancelled"
        } else if results.iter().any(|r| r.status == "failed") || e.error.is_some() {
            "completed_with_errors"
        } else {
            "completed"
        }
        .into();
        e.phase = if any {
            "操作结束，受影响目录已重新统计"
        } else {
            "操作结束"
        }
        .into();
        e.results = Some(results.clone());
    });
    results
}
fn refresh_affected(
    service: &Service,
    plan: &StoredPlan,
    attempted: &std::collections::HashSet<String>,
    job: &JobRuntime,
) -> Result<u32> {
    let c = service.repo.connect()?;
    let mut ids = std::collections::HashSet::new();
    for source in &plan.sources {
        if !attempted.contains(&source.stored.node.node_id) {
            continue;
        }
        if plan.dto.kind != "copy" {
            if let Some(parent) = &source.stored.node.parent_id {
                ids.insert(parent.clone());
            }
        }
        if let Some((target, _)) = &source.destination_parent {
            use rusqlite::OptionalExtension;
            let id: Option<i64> = c
                .query_row(
                    "SELECT id FROM nodes WHERE scan_id=?1 AND path=?2",
                    rusqlite::params![plan.dto.scan_id, platform::encode_path(target)],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = id {
                ids.insert(id.to_string());
            }
        }
    }
    let (roots, _) =
        Repository::normalize_conn(&c, &plan.dto.scan_id, &ids.into_iter().collect::<Vec<_>>())?;
    let mut revision = service.repo.scan(&plan.dto.scan_id)?.revision;
    if roots.is_empty() {
        return Ok(revision);
    }
    {
        let _writer = service.writer.lock().unwrap();
        let tx = c.unchecked_transaction()?;
        for root in &roots {
            tx.execute("WITH RECURSIVE subtree(id) AS (SELECT ?2 UNION ALL SELECT n.id FROM subtree s CROSS JOIN nodes n ON n.scan_id=?1 AND n.parent_id=s.id) UPDATE nodes SET stale=1 WHERE scan_id=?1 AND id IN subtree",rusqlite::params![plan.dto.scan_id,parse_id(&root.node.node_id)?])?;
        }
        tx.execute(
            "UPDATE scans SET stale=1,revision=revision+1 WHERE id=?1",
            [&plan.dto.scan_id],
        )?;
        tx.commit()?;
    }
    for root in roots {
        job.update(|e| e.phase = format!("重新统计受影响目录：{}", root.node.name));
        let staged = service.repo.new_scan_id()?;
        {
            let _writer = service.writer.lock().unwrap();
            service.repo.insert_scan(&staged, &root.path, true)?;
        }
        let inner = JobRuntime::new(JobEvent::new(&staged, &staged, "scan", 1), None);
        let scanned = crate::scan::scan_sync(service, &staged, &root.path, &inner)?;
        if !matches!(
            scanned.status.as_str(),
            "completed" | "completed_with_errors"
        ) {
            return Err(AppError::new("REFRESH_FAILED", "操作后目录未完成刷新"));
        }
        revision =
            crate::scan::replace_subtree(service, &plan.dto.scan_id, &root.node.node_id, &staged)?;
    }
    Ok(revision)
}
#[cfg(not(windows))]
fn recycle_preflight(_p: &Path) -> Result<()> {
    Err(AppError::new(
        "PLATFORM_UNSUPPORTED",
        "此首发版本的回收站操作仅支持 Windows",
    ))
}
#[cfg(not(windows))]
fn system_operation(
    _s: &PlannedSource,
    _kind: &str,
    _policy: &str,
    _job: &JobRuntime,
) -> Result<Option<PathBuf>> {
    Err(AppError::new(
        "PLATFORM_UNSUPPORTED",
        "此首发版本的文件管理仅支持 Windows",
    ))
}
#[cfg(windows)]
mod windows_ops;
#[cfg(windows)]
pub use windows_ops::{open_node, recycle_preflight, system_operation};
#[cfg(not(windows))]
pub fn open_node(_path: &Path, _action: &str) -> Result<()> {
    Err(AppError::new(
        "PLATFORM_UNSUPPORTED",
        "当前平台尚未实现系统关联动作",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_names() {
        for name in [
            "", "..", "a/b", "a\\b", "NUL.txt", "CON", "foo.", "foo ", "COM1",
        ] {
            assert!(validate_name(name).is_err(), "{name}");
        }
        assert!(validate_name("中文🧭.txt").is_ok());
    }
}
