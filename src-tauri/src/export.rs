use crate::{
    model::*,
    platform,
    repository::{read_node, Repository, NODE_COLUMNS},
    service::{JobRuntime, ProgressCallback, Service},
};
use rusqlite::{params, Connection};
use std::{
    collections::HashSet,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

pub const FIELDS: &[&str] = &[
    "name",
    "path",
    "logical_bytes",
    "kind",
    "parent_path",
    "depth",
    "own_logical_bytes",
    "allocated_bytes",
    "modified_at",
    "node_id",
    "parent_id",
    "measurement_status",
    "link_type",
    "error_code",
];
pub const XLSX_DATA_ROWS: u32 = 1_048_575;
pub fn worksheet_position(entry_index: u64) -> (u64, u32) {
    (
        entry_index / XLSX_DATA_ROWS as u64,
        (entry_index % XLSX_DATA_ROWS as u64) as u32 + 1,
    )
}
struct Resolved {
    count: i64,
    total: i64,
    scan: Scan,
}
fn validate(request: &ExportRequest) -> Result<()> {
    if !["csv", "xlsx", "json", "html"].contains(&request.format.as_str())
        || !["all", "current", "selected"].contains(&request.scope.as_str())
        || request.fields.is_empty()
        || request.fields.len() > FIELDS.len()
        || request.fields.iter().any(|f| !FIELDS.contains(&f.as_str()))
        || request.fields.iter().collect::<HashSet<_>>().len() != request.fields.len()
    {
        return Err(AppError::new("INVALID_EXPORT", "导出格式、范围或字段无效"));
    }
    Ok(())
}
fn resolve(c: &Connection, request: &ExportRequest) -> Result<Resolved> {
    validate(request)?;
    Repository::check_revision(c, &request.scan_id, request.revision)?;
    let roots = if request.scope == "selected" {
        if request.selected_ids.is_empty() {
            return Err(AppError::new("EMPTY_SELECTION", "请选择导出项目"));
        }
        Repository::normalize_conn(c, &request.scan_id, &request.selected_ids)?.0
    } else {
        vec![Repository::node_conn(
            c,
            &request.scan_id,
            if request.scope == "all" {
                1
            } else {
                parse_id(&request.scope_id)?
            },
        )?]
    };
    c.execute_batch("CREATE TEMP TABLE export_roots(id INTEGER PRIMARY KEY); CREATE TEMP TABLE export_rows(id INTEGER PRIMARY KEY); ")?;
    let mut total = 0i64;
    for root in &roots {
        c.execute(
            "INSERT INTO export_roots VALUES(?1)",
            [parse_id(&root.node.node_id)?],
        )?;
        total =
            crate::repository::checked_add(total, parse_bytes(&root.node.logical_total_bytes)?)?;
    }
    if request.scope == "all" && request.recursive {
        // All recursive rows stream directly from the scan primary key.
    } else if request.recursive {
        c.execute("WITH RECURSIVE subtree(id) AS (SELECT id FROM export_roots UNION ALL SELECT n.id FROM subtree p CROSS JOIN nodes n ON n.scan_id=?1 AND n.parent_id=p.id) INSERT OR IGNORE INTO export_rows SELECT id FROM subtree",[&request.scan_id])?;
    } else if request.scope == "selected" {
        c.execute("INSERT INTO export_rows SELECT id FROM export_roots", [])?;
    } else {
        c.execute("INSERT INTO export_rows SELECT n.id FROM nodes n JOIN export_roots r ON n.parent_id=r.id WHERE n.scan_id=?1",[&request.scan_id])?;
    }
    if request.scope != "selected" && request.recursive {
        c.execute("DELETE FROM export_rows WHERE id IN export_roots", [])?;
    }
    let count = if request.scope == "all" && request.recursive {
        c.query_row(
            "SELECT discovered-1 FROM scans WHERE id=?1",
            [&request.scan_id],
            |r| r.get(0),
        )?
    } else {
        c.query_row("SELECT COUNT(*) FROM export_rows", [], |r| r.get(0))?
    };
    Ok(Resolved {
        count,
        total,
        scan: Repository::scan_conn(c, &request.scan_id)?,
    })
}
pub fn row_value(node: &StoredNode, field: &str) -> serde_json::Value {
    let n = &node.node;
    let text = match field {
        "name" => Some(n.name.clone()),
        "path" => Some(n.display_path.clone()),
        "logical_bytes" => Some(n.logical_total_bytes.clone()),
        "kind" => Some(n.kind.clone()),
        "parent_path" => node.path.parent().map(platform::display),
        "depth" => Some(n.depth.to_string()),
        "own_logical_bytes" => Some(n.logical_bytes.clone()),
        "allocated_bytes" => n.allocated_bytes.clone(),
        "modified_at" => n.modified_at.clone(),
        "node_id" => Some(n.node_id.clone()),
        "parent_id" => n.parent_id.clone(),
        "measurement_status" => Some(
            if n.incomplete {
                "partial"
            } else if n.allocated_unknown_count != "0" {
                "allocation_partial"
            } else {
                "complete"
            }
            .into(),
        ),
        "link_type" => n.link_type.clone(),
        "error_code" => n.error_code.clone(),
        _ => None,
    };
    text.map(serde_json::Value::String)
        .unwrap_or(serde_json::Value::Null)
}
fn row_map(
    node: &StoredNode,
    request: &ExportRequest,
    json: bool,
) -> serde_json::Map<String, serde_json::Value> {
    let mut row = serde_json::Map::new();
    for field in &request.fields {
        row.insert(field.clone(), row_value(node, field));
    }
    if json {
        let (encoding, data, replaced) = platform::path_encoding(&node.path);
        row.insert("pathEncoding".into(), encoding.into());
        row.insert("pathData".into(), data.into());
        row.insert("displayPathReplaced".into(), replaced.into());
    }
    row
}
fn suggested_name(request: &ExportRequest) -> String {
    format!(
        "Spacefold-{}.{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        request.format
    )
}
pub fn preview(service: &Service, request: &ExportRequest) -> Result<ExportPreview> {
    let c = service.repo.connect()?;
    let tx = c.unchecked_transaction()?;
    let resolved = resolve(&tx, request)?;
    let clause = if request.scope == "all" && request.recursive {
        "id<>1"
    } else {
        "id IN (SELECT id FROM export_rows)"
    };
    let sql = format!(
        "SELECT {NODE_COLUMNS} FROM nodes WHERE scan_id=?1 AND {clause} ORDER BY id LIMIT 5"
    );
    let mut stmt = tx.prepare(&sql)?;
    let rows = stmt
        .query_map([&request.scan_id], read_node)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ExportPreview {
        record_count: resolved.count.to_string(),
        logical_total_bytes: resolved.total.to_string(),
        sample: rows.iter().map(|n| row_map(n, request, false)).collect(),
        suggested_name: suggested_name(request),
        incomplete: resolved.scan.incomplete || resolved.scan.stale,
    })
}
pub fn start(
    service: &Arc<Service>,
    request: ExportRequest,
    callback: Option<ProgressCallback>,
) -> Result<Job> {
    validate(&request)?;
    let target = PathBuf::from(
        request
            .target_path
            .as_deref()
            .ok_or_else(|| AppError::new("INVALID_TARGET", "请选择报告保存位置"))?,
    );
    start_with_target(service, request, target, callback)
}
pub fn start_with_target(
    service: &Arc<Service>,
    request: ExportRequest,
    target: PathBuf,
    callback: Option<ProgressCallback>,
) -> Result<Job> {
    validate(&request)?;
    if !target.is_absolute() {
        return Err(AppError::new("INVALID_TARGET", "报告目标必须是绝对路径"));
    }
    let parent = target
        .parent()
        .ok_or_else(|| AppError::new("INVALID_TARGET", "目标父目录无效"))?;
    let parent = platform::absolute(parent)?;
    let target = parent.join(
        target
            .file_name()
            .ok_or_else(|| AppError::new("INVALID_TARGET", "目标文件名无效"))?,
    );
    if target.starts_with(&service.repo.data_dir) {
        return Err(AppError::new(
            "APP_DATA_PROTECTED",
            "不能覆盖本应用的数据目录",
        ));
    }
    let _ = preview(service, &request)?;
    let id = uuid::Uuid::new_v4().to_string();
    let job = service.add_job(
        &id,
        JobEvent::new(&id, &request.scan_id, "export", request.revision),
        callback,
    )?;
    let response = Job {
        job_id: id.clone(),
        scan_id: request.scan_id.clone(),
    };
    let service = service.clone();
    std::thread::spawn(move || {
        let result = write_report(&service, &request, &target, &job).map_err(|error| {
            if job.cancel.load(Ordering::Acquire) {
                AppError::new("CANCELLED", "导出已取消，原目标文件已保留")
            } else {
                error
            }
        });
        match result {
            Ok(count) => job.update(|e| {
                e.status = "completed".into();
                e.written_count = count.to_string();
                e.output_path = Some(platform::display(&target));
                e.phase = "报告已保存".into()
            }),
            Err(error) => job.update(|e| {
                e.status = if error.code == "CANCELLED" {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                e.phase = if error.code == "CANCELLED" {
                    "导出已取消，原目标文件已保留"
                } else {
                    "导出失败，原目标文件已保留"
                }
                .into();
                e.error = Some(error);
            }),
        }
    });
    Ok(response)
}
fn cancelled(job: &JobRuntime) -> Result<()> {
    if job.cancel.load(Ordering::Acquire) {
        Err(AppError::new("CANCELLED", "导出已取消"))
    } else {
        Ok(())
    }
}
/// SQLite Backup API makes a bounded-on-disk consistent snapshot; no long export read pins the live WAL.
pub fn write_report(
    service: &Service,
    request: &ExportRequest,
    target: &Path,
    job: &JobRuntime,
) -> Result<u64> {
    validate(request)?;
    cancelled(job)?;
    job.update(|e| {
        e.status = "running".into();
        e.phase = "固定扫描版本".into()
    });
    let parent = target
        .parent()
        .ok_or_else(|| AppError::new("INVALID_TARGET", "目标目录无效"))?;
    if !parent.is_dir() {
        return Err(AppError::new("NOT_FOUND", "报告目标目录不存在"));
    }
    let normalized_parent = platform::absolute(parent)?;
    let normalized_target = normalized_parent.join(
        target
            .file_name()
            .ok_or_else(|| AppError::new("INVALID_TARGET", "目标文件名无效"))?,
    );
    if normalized_target.starts_with(&service.repo.data_dir) {
        return Err(AppError::new(
            "APP_DATA_PROTECTED",
            "不能覆盖本应用的索引/设置目录",
        ));
    }
    let target = normalized_target.as_path();
    let parent = normalized_parent.as_path();
    let target_parents = platform::parent_chain(target)?;
    let tempdir = tempfile::tempdir_in(&service.repo.data_dir)?;
    let snapshot_path = tempdir.path().join("snapshot.sqlite3");
    {
        let _writer = service.writer.lock().unwrap();
        let source = service.repo.connect()?;
        Repository::check_revision(&source, &request.scan_id, request.revision)?;
        let size: i64 = source.query_row(
            "SELECT page_count*page_size FROM pragma_page_count(),pragma_page_size()",
            [],
            |r| r.get(0),
        )?;
        if size > 8 * 1024 * 1024 * 1024 {
            return Err(AppError::new(
                "SNAPSHOT_BUDGET",
                "扫描数据库超过 8 GiB 一致副本预算，请缩小扫描范围",
            ));
        }
        let mut destination = Connection::open(&snapshot_path)?;
        let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
        loop {
            cancelled(job)?;
            match backup.step(256)? {
                rusqlite::backup::StepResult::Done => break,
                rusqlite::backup::StepResult::More => {}
                rusqlite::backup::StepResult::Busy | rusqlite::backup::StepResult::Locked => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                _ => return Err(AppError::new("SNAPSHOT_ERROR", "一致副本未能完成")),
            }
        }
    }
    cancelled(job)?;
    let c = Connection::open(&snapshot_path)?;
    let cancel = job.cancel.clone();
    c.progress_handler(10_000, Some(move || cancel.load(Ordering::Acquire)))?;
    c.execute_batch("PRAGMA temp_store=FILE; PRAGMA cache_size=-8192;")?;
    let resolved = resolve(&c, request).map_err(|error| {
        if job.cancel.load(Ordering::Acquire) {
            AppError::new("CANCELLED", "范围解析已取消")
        } else {
            error
        }
    })?;
    if request.format == "html"
        && resolved.count > request.html_row_budget.unwrap_or(50_000).min(200_000) as i64
    {
        return Err(AppError::new(
            "HTML_ROW_BUDGET",
            "HTML 报告超过记录预算，请缩小范围或改用 CSV / JSON",
        ));
    }
    let metadata = serde_json::json!({"product":"Spacefold","schemaVersion":1,"source":resolved.scan.source,"scanId":request.scan_id,"revision":request.revision,"startedAt":resolved.scan.started_at,"finishedAt":resolved.scan.finished_at,"scanStatus":resolved.scan.status,"incomplete":resolved.scan.incomplete,"stale":resolved.scan.stale,"errorCount":resolved.scan.error_count,"metric":"logical_per_directory_entry","scope":request.scope,"scopeId":request.scope_id,"recursive":request.recursive,"totalLogicalBytes":resolved.total.to_string(),"recordCount":resolved.count.to_string(),"unit":"bytes","coverageNotes":resolved.scan.coverage_notes,"textHandling":if request.format=="csv"{"Spreadsheet text prefixes may be altered with an apostrophe; choose JSON for lossless paths and bytes."}else{"Display strings may replace invalid Unicode; JSON carries original pathData."},"precision":"Byte values are decimal strings; directory totals must not be summed with descendant rows."});
    job.update(|e| e.phase = "核验输出目录".into());
    platform::verify_parents(&target_parents)?;
    let mut temp = report_tempfile(parent)?;
    let mut cleanup = ReportTempCleanup {
        file: temp.as_file().try_clone()?,
        committed: false,
    };
    let temp_path = std::fs::canonicalize(temp.path())?;
    service.work_files.lock().unwrap().insert(temp_path.clone());
    struct WorkGuard<'a> {
        service: &'a Service,
        path: PathBuf,
    }
    impl Drop for WorkGuard<'_> {
        fn drop(&mut self) {
            self.service.work_files.lock().unwrap().remove(&self.path);
        }
    }
    let _guard = WorkGuard {
        service,
        path: temp_path,
    };
    job.update(|e| {
        e.phase = "流式写入报告".into();
        e.logical_total_bytes = resolved.total.to_string()
    });
    let count = match request.format.as_str() {
        "csv" => write_csv(&c, request, &metadata, temp.as_file_mut(), job)?,
        "json" => write_json(&c, request, &metadata, temp.as_file_mut(), job)?,
        "html" => write_html(&c, request, &metadata, temp.as_file_mut(), job)?,
        "xlsx" => write_xlsx(
            &c,
            request,
            &metadata,
            temp.as_file_mut(),
            tempdir.path(),
            job,
        )?,
        _ => return Err(AppError::new("INVALID_EXPORT", "格式无效")),
    };
    if count != resolved.count as u64 {
        return Err(AppError::new(
            "EXPORT_COUNT_MISMATCH",
            "报告记录数量核验失败",
        ));
    }
    cancelled(job)?;
    temp.as_file_mut().flush()?;
    temp.as_file().sync_all()?;
    cancelled(job)?;
    platform::verify_parents(&target_parents)?;
    // NamedTempFile::persist uses an atomic replacement on the same volume. The prior file survives every earlier failure/cancellation.
    temp.persist(target).map_err(|e| AppError::from(e.error))?;
    cleanup.committed = true;
    Ok(count)
}
fn report_tempfile(parent: &Path) -> Result<tempfile::NamedTempFile> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::*;
        Ok(tempfile::Builder::new()
            .prefix(".tmp")
            .make_in(parent, |path| {
                std::fs::OpenOptions::new()
                    .create_new(true)
                    .read(true)
                    .write(true)
                    .access_mode(FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0 | DELETE.0)
                    .custom_flags(FILE_ATTRIBUTE_TEMPORARY.0)
                    .open(path)
            })?)
    }
    #[cfg(not(windows))]
    {
        Ok(tempfile::NamedTempFile::new_in(parent)?)
    }
}
struct ReportTempCleanup {
    file: std::fs::File,
    committed: bool,
}
impl Drop for ReportTempCleanup {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::{Foundation::HANDLE, Storage::FileSystem::*};
            // Delete only our opened temporary object, even after a parent rename.
            // DELETE access was captured at creation, before any ACL/path changes.
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            unsafe {
                let _ = SetFileInformationByHandle(
                    HANDLE(self.file.as_raw_handle()),
                    FileDispositionInfo,
                    &disposition as *const _ as *const _,
                    std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
                );
            }
        }
    }
}
fn walk_rows(
    c: &Connection,
    request: &ExportRequest,
    job: &JobRuntime,
    mut consume: impl FnMut(u64, StoredNode) -> Result<()>,
) -> Result<u64> {
    let clause = if request.scope == "all" && request.recursive {
        "id<>1"
    } else {
        "EXISTS(SELECT 1 FROM export_rows WHERE export_rows.id=nodes.id)"
    };
    let sql=format!("SELECT {NODE_COLUMNS} FROM nodes WHERE scan_id=?1 AND id>?2 AND {clause} ORDER BY id LIMIT 500");
    let mut offset = 0u64;
    let mut last_id = 0i64;
    let mut last_emit = Instant::now();
    loop {
        cancelled(job)?;
        let mut stmt = c.prepare(&sql)?;
        let rows = stmt.query_map(params![request.scan_id, last_id], read_node)?;
        let mut count = 0u64;
        for row in rows {
            cancelled(job)?;
            let node = row?;
            last_id = parse_id(&node.node.node_id)?;
            consume(offset + count, node)?;
            count += 1;
        }
        offset += count;
        if last_emit.elapsed() >= Duration::from_millis(100) {
            job.update(|e| e.written_count = offset.to_string());
            last_emit = Instant::now();
        }
        if count < 500 {
            break;
        }
    }
    Ok(offset)
}
pub fn spreadsheet_text(value: &str) -> String {
    let first = value
        .trim_start_matches(|c: char| c.is_whitespace() || c.is_control())
        .chars()
        .next();
    let danger = first
        .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@' | '＝' | '＋' | '－' | '＠'))
        || value.chars().next().is_some_and(char::is_control);
    if danger {
        format!("'{value}")
    } else {
        value.to_owned()
    }
}
fn write_csv(
    c: &Connection,
    request: &ExportRequest,
    metadata: &serde_json::Value,
    file: &mut std::fs::File,
    job: &JobRuntime,
) -> Result<u64> {
    file.write_all(&[0xef, 0xbb, 0xbf])?;
    let mut writer = csv::WriterBuilder::new().from_writer(file);
    let header = std::iter::once("record_type")
        .chain(std::iter::once("metadata_json"))
        .chain(request.fields.iter().map(String::as_str));
    writer.write_record(header)?;
    let mut metarow = vec!["metadata".into(), serde_json::to_string(metadata)?];
    metarow.extend(request.fields.iter().map(|_| String::new()));
    writer.write_record(metarow)?;
    let count = walk_rows(c, request, job, |_, node| {
        let mut row = vec!["entry".to_owned(), String::new()];
        for f in &request.fields {
            let value = row_value(&node, f);
            row.push(if let Some(v) = value.as_str() {
                if matches!(
                    f.as_str(),
                    "name" | "path" | "parent_path" | "link_type" | "error_code"
                ) {
                    spreadsheet_text(v)
                } else {
                    v.into()
                }
            } else {
                String::new()
            });
        }
        writer.write_record(row)?;
        Ok(())
    })?;
    writer.flush()?;
    Ok(count)
}
fn write_json(
    c: &Connection,
    request: &ExportRequest,
    metadata: &serde_json::Value,
    file: &mut std::fs::File,
    job: &JobRuntime,
) -> Result<u64> {
    let mut writer = BufWriter::new(file);
    write!(
        writer,
        "{{\"schemaVersion\":1,\"metadata\":{},\"rows\":[",
        serde_json::to_string(metadata)?
    )?;
    let count = walk_rows(c, request, job, |index, node| {
        if index > 0 {
            writer.write_all(b",")?;
        }
        serde_json::to_writer(&mut writer, &row_map(&node, request, true))?;
        Ok(())
    })?;
    writer.write_all(b"]}")?;
    writer.flush()?;
    Ok(count)
}
pub fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn write_html(
    c: &Connection,
    request: &ExportRequest,
    metadata: &serde_json::Value,
    file: &mut std::fs::File,
    job: &JobRuntime,
) -> Result<u64> {
    let mut writer = BufWriter::new(file);
    writer.write_all("<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>Spacefold 扫描报告</title><style>body{font:14px system-ui,sans-serif;margin:32px;color:#242235;background:#faf9fc}h1{color:#7554ca}pre{white-space:pre-wrap;overflow-wrap:anywhere;background:#eeebf6;padding:16px;border-radius:12px}table{border-collapse:collapse;width:100%}th,td{text-align:left;padding:8px;border-bottom:1px solid #ddd;overflow-wrap:anywhere}th{position:sticky;top:0;background:#eeebf6}td{max-width:420px}</style><h1>间隙 Spacefold · 扫描报告</h1><p>大小为逻辑字节，硬链接按路径统计。目录与子文件的递归大小不能重复相加。扫描不是文件系统原子快照。</p><h2>概览与完整性</h2><pre>".as_bytes())?;
    writer.write_all(html_escape(&serde_json::to_string_pretty(metadata)?).as_bytes())?;
    writer.write_all(b"</pre><h2>\xE6\x95\xB0\xE6\x8D\xAE</h2><table><thead><tr>")?;
    for field in &request.fields {
        write!(writer, "<th>{}</th>", html_escape(field))?;
    }
    writer.write_all(b"</tr></thead><tbody>")?;
    let count = walk_rows(c, request, job, |_, node| {
        writer.write_all(b"<tr>")?;
        for field in &request.fields {
            let value = row_value(&node, field);
            write!(
                writer,
                "<td>{}</td>",
                html_escape(value.as_str().unwrap_or("—"))
            )?;
        }
        writer.write_all(b"</tr>")?;
        Ok(())
    })?;
    writer.write_all(b"</tbody></table></html>")?;
    writer.flush()?;
    Ok(count)
}
fn write_xlsx(
    c: &Connection,
    request: &ExportRequest,
    metadata: &serde_json::Value,
    file: &mut std::fs::File,
    tempdir: &Path,
    job: &JobRuntime,
) -> Result<u64> {
    use rust_xlsxwriter::{Format, Workbook};
    let mut workbook = Workbook::new();
    workbook.set_tempdir(tempdir)?;
    let header = Format::new().set_bold().set_background_color("EEEAF8");
    let overview = workbook.add_worksheet();
    overview.set_name("概览")?;
    overview.set_column_width(0, 28)?;
    overview.set_column_width(1, 92)?;
    overview.write_string_with_format(0, 0, "Spacefold 扫描报告", &header)?;
    for (i, (key, value)) in metadata.as_object().unwrap().iter().enumerate() {
        overview.write_string(i as u32 + 1, 0, key)?;
        let text = value
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| value.to_string());
        if text.encode_utf16().count() > 32767 {
            return Err(AppError::new(
                "FORMAT_LIMIT",
                "XLSX 概览字段超过 32767 字符，请改用 JSON",
            ));
        }
        overview.write_string(i as u32 + 1, 1, text)?;
    }
    let mut active_sheet = u64::MAX;
    let count = walk_rows(c, request, job, |index, node| {
        let (sheet, row) = worksheet_position(index);
        if sheet != active_sheet {
            active_sheet = sheet;
            let ws = workbook.add_worksheet_with_constant_memory();
            ws.set_name(format!("数据{}", sheet + 1))?;
            ws.set_freeze_panes(1, 0)?;
            for (col, field) in request.fields.iter().enumerate() {
                ws.set_column_width(
                    col as u16,
                    if matches!(field.as_str(), "path" | "parent_path") {
                        64.0
                    } else {
                        24.0
                    },
                )?;
                ws.write_string_with_format(0, col as u16, field, &header)?;
            }
            ws.autofilter(
                0,
                0,
                XLSX_DATA_ROWS.min(
                    (metadata["recordCount"]
                        .as_str()
                        .unwrap_or("0")
                        .parse::<u64>()
                        .unwrap_or(0)
                        .saturating_sub(sheet * XLSX_DATA_ROWS as u64)) as u32,
                ),
                request.fields.len() as u16 - 1,
            )?;
        }
        let ws = workbook.worksheet_from_name(&format!("数据{}", sheet + 1))?;
        for (col, field) in request.fields.iter().enumerate() {
            let value = row_value(&node, field);
            if let Some(text) = value.as_str() {
                if text.encode_utf16().count() > 32767 {
                    return Err(AppError::new(
                        "FORMAT_LIMIT",
                        format!("XLSX 字段 {field} 超过 32767 字符，请改用 JSON"),
                    ));
                }
                ws.write_string(row, col as u16, text)?;
            }
        }
        Ok(())
    })?;
    if count == 0 {
        let ws = workbook.add_worksheet_with_constant_memory();
        ws.set_name("数据1")?;
        ws.set_freeze_panes(1, 0)?;
        for (col, field) in request.fields.iter().enumerate() {
            ws.write_string_with_format(0, col as u16, field, &header)?;
        }
    }
    cancelled(job)?;
    job.update(|e| e.phase = "封装 OOXML 工作簿".into());
    struct CancelWriter<'a> {
        file: &'a mut std::fs::File,
        job: &'a JobRuntime,
    }
    impl CancelWriter<'_> {
        fn check(&self) -> std::io::Result<()> {
            if self.job.cancel.load(Ordering::Acquire) {
                Err(std::io::Error::other("CANCELLED"))
            } else {
                Ok(())
            }
        }
    }
    impl std::io::Write for CancelWriter<'_> {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.check()?;
            self.file.write(buffer)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.check()?;
            self.file.flush()
        }
    }
    impl std::io::Seek for CancelWriter<'_> {
        fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
            self.check()?;
            std::io::Seek::seek(self.file, pos)
        }
    }
    workbook.save_to_writer(CancelWriter { file, job })?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formula_and_html() {
        for s in ["=1+1", "\t@x", "  ＋foo", "-12", "\rname"] {
            assert!(spreadsheet_text(s).starts_with('\''));
        }
        assert_eq!(spreadsheet_text("hello"), "hello");
        assert_eq!(html_escape("<a & \"b\">"), "&lt;a &amp; &quot;b&quot;&gt;");
    }
    #[test]
    fn excel_boundary() {
        assert_eq!(worksheet_position(0), (0, 1));
        assert_eq!(worksheet_position(1_048_574), (0, 1_048_575));
        assert_eq!(worksheet_position(1_048_575), (1, 1));
    }
}
