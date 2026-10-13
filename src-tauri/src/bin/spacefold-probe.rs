use spacefold_lib::{
    export,
    model::*,
    platform,
    repository::{insert_node, Repository},
    service::{JobRuntime, ProgressCallback, Service},
};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
fn output(value: impl serde::Serialize) {
    println!("{}", serde_json::to_string(&value).unwrap());
}
fn main() {
    if let Err(error) = run() {
        output(error);
        std::process::exit(1)
    }
}
fn finished_scan(service: &Service, scan_id: &str, event: &JobEvent) -> Result<Scan> {
    if let Some(error) = &event.error {
        return Err(error.clone());
    }
    if event.status == "failed" {
        return Err(AppError::new("SCAN_FAILED", event.phase.clone()));
    }
    // Close failure can change an earlier completed row into failed. Never
    // return a snapshot taken before the authoritative runtime termination.
    let current = service.repo.scan(scan_id)?;
    if current.status != event.status {
        return Err(AppError::new(
            "JOB_STATE_INCONSISTENT",
            "扫描持久状态与最终事件不一致",
        ));
    }
    Ok(current)
}
struct ScanTerminal {
    event: JobEvent,
    callback_elapsed_ms: f64,
}
struct ScanNotifications {
    callback: ProgressCallback,
    terminal: crossbeam_channel::Receiver<ScanTerminal>,
    first_result_elapsed_ms: Arc<Mutex<Option<u128>>>,
}
impl ScanNotifications {
    fn new(started: Instant) -> Self {
        let (sender, terminal) = crossbeam_channel::bounded(1);
        let first_result_elapsed_ms = Arc::new(Mutex::new(None));
        let first = first_result_elapsed_ms.clone();
        let sent = AtomicBool::new(false);
        let callback = Arc::new(move |event: JobEvent| {
            let is_terminal = matches!(
                event.status.as_str(),
                "completed" | "completed_with_errors" | "cancelled" | "failed" | "interrupted"
            );
            let callback_elapsed_ms = is_terminal.then(|| started.elapsed().as_secs_f64() * 1000.0);
            if event.discovered_count.parse::<u64>().unwrap_or(0) > 1 {
                if let Ok(mut first) = first.lock() {
                    if first.is_none() {
                        *first = Some(started.elapsed().as_millis());
                    }
                }
            }
            if let Some(callback_elapsed_ms) = callback_elapsed_ms {
                if !sent.swap(true, Ordering::AcqRel) {
                    // One owned terminal notification, even if it precedes
                    // start_scan's return. Never block the scanner callback.
                    let _ = sender.try_send(ScanTerminal {
                        event,
                        callback_elapsed_ms,
                    });
                }
            }
        });
        Self {
            callback,
            terminal,
            first_result_elapsed_ms,
        }
    }
    fn wait(&self) -> Result<ScanTerminal> {
        self.terminal
            .recv()
            .map_err(|_| AppError::new("SCAN_NOTIFICATION_CLOSED", "扫描终态通知通道已关闭"))
    }
}
fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("version") {
        output(
            serde_json::json!({"sqliteVersion":rusqlite::version(),"sqliteVersionNumber":rusqlite::version_number(),"sqliteThreadsafe":unsafe{rusqlite::ffi::sqlite3_threadsafe()},"sqliteCompileOptions":{"defaultMemstatus0":unsafe{rusqlite::ffi::sqlite3_compileoption_used(c"DEFAULT_MEMSTATUS=0".as_ptr())!=0},"threadsafe1":unsafe{rusqlite::ffi::sqlite3_compileoption_used(c"THREADSAFE=1".as_ptr())!=0},"enableApiArmor":unsafe{rusqlite::ffi::sqlite3_compileoption_used(c"ENABLE_API_ARMOR".as_ptr())!=0}}}),
        );
        return Ok(());
    }
    if args.len() < 2 {
        return Err(AppError::new("USAGE","scan DATA_DIR SOURCE | export DATA_DIR SCAN_ID FORMAT TARGET | synthetic DATA_DIR COUNT wide|deep | inspect DATA_DIR SCAN_ID [PARENT_ID] [CURSOR]"));
    }
    if args[0] == "kernel" {
        return kernel(&PathBuf::from(&args[1]));
    }
    let service = Service::new(&PathBuf::from(&args[1]))?;
    match args[0].as_str() {
        "database-info" => {
            let c = service.repo.connect()?;
            let filename: String = c.query_row("PRAGMA database_list", [], |r| r.get(2))?;
            output(
                serde_json::json!({"databaseFilename":filename,"pageSize":c.query_row("PRAGMA page_size",[],|r|r.get::<_,i64>(0))?,"journalMode":c.query_row("PRAGMA journal_mode",[],|r|r.get::<_,String>(0))?}),
            );
        }
        #[cfg(feature = "scan-reference")]
        "scan-reference" => {
            let elapsed = Instant::now();
            let root = platform::absolute(&PathBuf::from(
                args.get(2)
                    .ok_or_else(|| AppError::new("USAGE", "SOURCE required"))?,
            ))?;
            let id = service.repo.new_scan_id()?;
            service.repo.insert_scan(&id, &root, false)?;
            let job = JobRuntime::new(JobEvent::new(&id, &id, "scan", 1), None);
            let scan = spacefold_lib::scan_reference::scan_sync(&service, &id, &root, &job)?;
            output(
                serde_json::json!({"scan":scan,"elapsedMs":elapsed.elapsed().as_millis(),"mode":"test_only_synchronous_reference"}),
            );
        }
        "storage" => {
            let c = service.repo.connect()?;
            let mut statement = c.prepare("SELECT name,COUNT(*),SUM(pgsize),SUM(payload),SUM(unused) FROM dbstat GROUP BY name ORDER BY SUM(pgsize) DESC")?;
            let tables = statement.query_map([], |row| Ok(serde_json::json!({"name":row.get::<_,String>(0)?,"pages":row.get::<_,i64>(1)?,"bytes":row.get::<_,i64>(2)?,"payloadBytes":row.get::<_,i64>(3)?,"unusedBytes":row.get::<_,i64>(4)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
            output(
                serde_json::json!({"pageSize":c.query_row("PRAGMA page_size",[],|r|r.get::<_,i64>(0))?,"pageCount":c.query_row("PRAGMA page_count",[],|r|r.get::<_,i64>(0))?,"tables":tables}),
            );
        }
        "scan" => {
            let elapsed = Instant::now();
            let notifications = ScanNotifications::new(elapsed);
            let root = platform::absolute(&PathBuf::from(
                args.get(2)
                    .ok_or_else(|| AppError::new("USAGE", "SOURCE required"))?,
            ))?;
            let scan = service.start_scan(root, Some(notifications.callback.clone()))?;
            let terminal = notifications.wait()?;
            let current = finished_scan(&service, &scan.scan_id, &terminal.event)?;
            output(serde_json::json!({
                "scan":current,
                // Existing command timing still includes authoritative final
                // reread; the new callback field freezes before that reread.
                "elapsedMs":elapsed.elapsed().as_millis(),
                "firstResultElapsedMs":*notifications.first_result_elapsed_ms.lock().unwrap(),
                "callbackElapsedMs":terminal.callback_elapsed_ms,
                "nativeElapsedMs":terminal.event.elapsed_ms,
                "timingOrigins":{
                    "elapsedMs":"scan command after Service initialization through terminal notification and authoritative final reread; integer milliseconds",
                    "firstResultElapsedMs":"first existing callback with discoveredCount greater than one, from the scan command origin; integer milliseconds",
                    "callbackElapsedMs":"scan command origin to entry into the existing terminal callback, before final reread; monotonic fractional milliseconds",
                    "nativeElapsedMs":"production JobRuntime origin to terminal event after scanner resource cleanup; integer milliseconds"
                }
            }));
        }
        "export" => {
            if args.len() < 5 {
                return Err(AppError::new(
                    "USAGE",
                    "export DATA_DIR SCAN_ID FORMAT TARGET",
                ));
            }
            let scan = service.repo.scan(&args[2])?;
            let request = ExportRequest {
                scan_id: scan.scan_id.clone(),
                revision: scan.revision,
                scope: "all".into(),
                scope_id: "1".into(),
                selected_ids: vec![],
                recursive: true,
                fields: if args.get(5).map(String::as_str) == Some("minimal") {
                    vec!["node_id".into(), "logical_bytes".into()]
                } else {
                    export::FIELDS.iter().map(|s| s.to_string()).collect()
                },
                format: args[3].clone(),
                target_path: Some(args[4].clone()),
                html_row_budget: None,
            };
            let start = Instant::now();
            let job = JobRuntime::new(
                JobEvent::new("probe", &scan.scan_id, "export", scan.revision),
                None,
            );
            let count = export::write_report(&service, &request, &PathBuf::from(&args[4]), &job)?;
            output(
                serde_json::json!({"count":count.to_string(),"elapsedMs":start.elapsed().as_millis(),"path":args[4],"logicalTotalBytes":scan.logical_total_bytes}),
            );
        }
        "synthetic" => {
            let count = args
                .get(2)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(1_000_000);
            if count > 2_000_000 {
                return Err(AppError::new(
                    "SYNTHETIC_LIMIT",
                    "最多 2000000 条测试元数据",
                ));
            }
            let mode = args.get(3).map(String::as_str).unwrap_or("wide");
            let (start, scan) = (Instant::now(), synthetic(&service, count, mode)?);
            let page_start = Instant::now();
            let first = service.repo.page(
                &PageRequest {
                    scan_id: scan.scan_id.clone(),
                    parent_id: Some("1".into()),
                    revision: scan.revision,
                    sort: None,
                    filter: None,
                    cursor: None,
                    page_size: Some(200),
                },
                false,
            )?;
            let first_ms = page_start.elapsed().as_millis();
            let page_start = Instant::now();
            let last = service.repo.page(
                &PageRequest {
                    scan_id: scan.scan_id.clone(),
                    parent_id: Some("1".into()),
                    revision: scan.revision,
                    sort: None,
                    filter: None,
                    cursor: if mode == "wide" && count > 200 {
                        Some(last_cursor(&service, &scan)?)
                    } else {
                        None
                    },
                    page_size: Some(200),
                },
                false,
            )?;
            let last_ms = page_start.elapsed().as_millis();
            let deep_id = if mode == "deep" && count >= 64 {
                "33"
            } else {
                "1"
            };
            let deep_start = Instant::now();
            let deep = service.repo.page(
                &PageRequest {
                    scan_id: scan.scan_id.clone(),
                    parent_id: Some(deep_id.into()),
                    revision: scan.revision,
                    sort: None,
                    filter: None,
                    cursor: None,
                    page_size: Some(200),
                },
                false,
            )?;
            let ancestors = service
                .repo
                .ancestors(&scan.scan_id, deep_id, scan.revision)?;
            output(
                serde_json::json!({"scan":scan,"mode":mode,"createElapsedMs":start.elapsed().as_millis(),"firstPageMs":first_ms,"lastPageMs":last_ms,"firstPageCount":first.nodes.len(),"lastPageCount":last.nodes.len(),"deepPageCount":deep.nodes.len(),"ancestorCount":ancestors.len(),"deepPageMs":deep_start.elapsed().as_millis()}),
            );
        }
        "inspect" => {
            let scan = service.repo.scan(
                args.get(2)
                    .ok_or_else(|| AppError::new("USAGE", "SCAN_ID required"))?,
            )?;
            let cursor = if args.get(4).map(String::as_str) == Some("last") {
                Some(last_cursor(&service, &scan)?)
            } else {
                args.get(4).cloned()
            };
            let page_start = Instant::now();
            let page = service.repo.page(
                &PageRequest {
                    scan_id: scan.scan_id.clone(),
                    parent_id: Some(args.get(3).cloned().unwrap_or("1".into())),
                    revision: scan.revision,
                    sort: None,
                    filter: args.get(5).map(|kind| Filter {
                        kind: Some(kind.clone()),
                        ..Default::default()
                    }),
                    cursor,
                    page_size: Some(200),
                },
                false,
            )?;
            let page_ms = page_start.elapsed().as_millis();
            let ancestors = service.repo.ancestors(
                &scan.scan_id,
                args.get(3).map(String::as_str).unwrap_or("1"),
                scan.revision,
            )?;
            output(
                serde_json::json!({"pageElapsedMs":page_ms,"count":page.nodes.len(),"firstId":page.nodes.first().map(|n|n.node_id.clone()),"lastId":page.nodes.last().map(|n|n.node_id.clone()),"ancestorCount":ancestors.len(),"page":page}),
            );
        }
        "page" => {
            let request: PageRequest = serde_json::from_str(
                args.get(2)
                    .ok_or_else(|| AppError::new("USAGE", "page DATA_DIR REQUEST_JSON"))?,
            )?;
            let start = Instant::now();
            let page = service.repo.page(&request, false)?;
            output(
                serde_json::json!({"pageElapsedMs":start.elapsed().as_secs_f64()*1000.0,"page":page}),
            );
        }
        "node-page" => {
            let scan_id = args.get(2).ok_or_else(|| {
                AppError::new(
                    "USAGE",
                    "node-page DATA_DIR SCAN_ID NODE_ID SORT [PAGE_SIZE]",
                )
            })?;
            let node_id = args
                .get(3)
                .ok_or_else(|| AppError::new("USAGE", "NODE_ID required"))?;
            let sort = args.get(4).map(String::as_str).unwrap_or("size_desc");
            let size = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(200);
            let scan = service.repo.scan(scan_id)?;
            let start = Instant::now();
            let page = service
                .repo
                .node_page(scan_id, node_id, scan.revision, sort, size)?;
            output(
                serde_json::json!({"pageElapsedMs":start.elapsed().as_secs_f64()*1000.0,"page":page}),
            );
        }
        _ => return Err(AppError::new("USAGE", "未知 probe 命令")),
    }
    Ok(())
}
fn last_cursor(service: &Service, scan: &Scan) -> Result<String> {
    let c = service.repo.connect()?;
    let(id,name,size,modified):(i64,String,i64,Option<String>)=c.query_row("SELECT id,name,logical_total,modified FROM nodes WHERE scan_id=?1 AND parent_id=1 ORDER BY sort_size DESC,name COLLATE NOCASE DESC,id DESC LIMIT 1 OFFSET 200",[&scan.scan_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    let key = spacefold_lib::repository::page_cursor_key(
        &scan.scan_id,
        scan.revision,
        Some(1),
        "size_desc",
        &Filter::default(),
    )?;
    Ok(serde_json::json!({"key":key,"id":id,"name":name,"size":size,"modified":modified.unwrap_or_default()}).to_string())
}

fn kernel(root: &std::path::Path) -> Result<()> {
    let root = platform::absolute(root)?;
    let start = Instant::now();
    let meta = platform::metadata(&root)?;
    if !meta.recurse {
        return Err(AppError::new("UNSAFE_SCAN_ROOT", "不能枚举此根目录"));
    }
    let mut stack = vec![platform::read_dir(&root)?];
    let (mut count, mut bytes, mut errors) = (0u64, 0i64, 0u64);
    while let Some(iter) = stack.last_mut() {
        match iter.next() {
            None => {
                stack.pop();
            }
            Some(Err(_)) => errors += 1,
            Some(Ok(entry)) => {
                count += 1;
                let meta = entry
                    .hint
                    .map(Ok)
                    .unwrap_or_else(|| platform::metadata(&entry.path));
                match meta {
                    Err(_) => errors += 1,
                    Ok(meta) => {
                        bytes = bytes
                            .checked_add(integer(meta.logical)?)
                            .ok_or_else(|| AppError::new("SIZE_OUT_OF_RANGE", "扫描合计溢出"))?;
                        if meta.recurse {
                            match platform::read_dir(&entry.path) {
                                Ok(iter) => stack.push(iter),
                                Err(_) => errors += 1,
                            }
                        }
                    }
                }
            }
        }
    }
    output(
        serde_json::json!({"count":count.to_string(),"logicalTotalBytes":bytes.to_string(),"errors":errors.to_string(),"elapsedMs":start.elapsed().as_millis(),"mode":"kernel_without_persistence"}),
    );
    Ok(())
}
fn synthetic(service: &Service, count: u64, mode: &str) -> Result<Scan> {
    if !["wide", "deep"].contains(&mode) {
        return Err(AppError::new("USAGE", "mode must be wide or deep"));
    }
    let root = service.repo.data_dir.join("synthetic-source");
    fs::create_dir_all(&root)?;
    let root = platform::absolute(&root)?;
    let id = service.repo.new_scan_id()?;
    service.repo.insert_scan(&id, &root, false)?;
    let mut c = service.repo.connect()?;
    let tx = c.transaction()?;
    let meta = platform::metadata(&root)?;
    insert_node(&tx, &id, 1, None, &root, 0, &meta, None)?;
    tx.execute("DELETE FROM pending_dirs WHERE scan_id=?1", [&id])?;
    tx.commit()?;
    let mut template = Repository::node_conn(&c, &id, 1)?.node;
    template.identity = None;
    template.link_count = None;
    template.modified_at = None;
    template.incomplete = false;
    template.allocation_method = Some("synthetic_read_only".into());
    let mut total = 0i64;
    let mut files = 0i64;
    let mut dirs = 0i64;
    for chunk in (0..count).step_by(2000) {
        let tx = c.transaction()?;
        {
            let mut insert=tx.prepare_cached("INSERT INTO nodes(scan_id,id,parent_id,path,payload,name,kind,extension,depth,logical_total,allocated_total,allocated_unknown,file_count,dir_count,children_count,incomplete) VALUES(?1,?2,?3,?4,?5,?6,?7,'bin',?8,?9,?9,0,?10,?11,?12,0)")?;
            for i in chunk..count.min(chunk + 2000) {
                let nid = i as i64 + 2;
                let group = i / 64;
                let index = i % 64;
                let group_size = (count - group * 64).min(64);
                let isdir = mode == "deep" && index < 32;
                let leaves = group_size.saturating_sub(32);
                let leaf_bytes = if mode == "deep" {
                    100_000_000_000i64
                } else {
                    10_000_000_000i64
                };
                let parent = if mode == "wide" || index == 0 {
                    1
                } else if isdir {
                    nid - 1
                } else {
                    group as i64 * 64 + 33
                };
                let name = if isdir {
                    format!("directory-{group}-{index}")
                } else {
                    format!("file-{i:07}.bin")
                };
                let mut path = root.clone();
                if mode == "deep" {
                    for depth in 0..if isdir { index } else { 32 } {
                        path.push(format!("directory-{group}-{depth}"));
                    }
                }
                path.push(&name);
                let depth = if mode == "wide" {
                    1
                } else if isdir {
                    index as u32 + 1
                } else {
                    33
                };
                let value = if isdir {
                    leaves as i64 * leaf_bytes
                } else {
                    leaf_bytes
                };
                let fc = if isdir { leaves as i64 } else { 1 };
                let dc = if isdir {
                    group_size.min(32).saturating_sub(index + 1) as i64
                } else {
                    0
                };
                let children = if isdir {
                    if index == 31 {
                        leaves as i64
                    } else if index + 1 < group_size {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                };
                let mut node = template.clone();
                node.node_id = nid.to_string();
                node.parent_id = Some(parent.to_string());
                node.name = name.clone();
                node.display_path = platform::display(&path);
                node.kind = if isdir { "directory" } else { "file" }.into();
                node.depth = depth;
                node.extension = if isdir { String::new() } else { "bin".into() };
                node.logical_bytes = if isdir { 0 } else { leaf_bytes }.to_string();
                node.logical_total_bytes = value.to_string();
                node.allocated_bytes = Some(node.logical_bytes.clone());
                node.allocated_known_total_bytes = value.to_string();
                node.file_count = fc.to_string();
                node.directory_count = dc.to_string();
                node.children_count = children.to_string();
                let fp = Fingerprint {
                    identity: None,
                    kind: node.kind.clone(),
                    logical: if isdir { 0 } else { leaf_bytes as u64 },
                    modified: None,
                    attributes: 0,
                    reparse_tag: 0,
                };
                let payload = serde_json::json!({"node":node,"fingerprint":fp});
                insert.execute(rusqlite::params![
                    id,
                    nid,
                    parent,
                    platform::encode_path(&path),
                    serde_json::to_string(&payload)?,
                    name,
                    if isdir { "directory" } else { "file" },
                    depth,
                    value,
                    fc,
                    dc,
                    children
                ])?;
                if isdir {
                    dirs += 1
                } else {
                    files += 1;
                    total = total
                        .checked_add(leaf_bytes)
                        .ok_or_else(|| AppError::new("SIZE_OUT_OF_RANGE", "合成字节溢出"))?;
                }
            }
        }
        tx.commit()?;
    }
    c.execute("UPDATE nodes SET logical_total=?2,allocated_total=?2,file_count=?3,dir_count=?4,children_count=?5,incomplete=0 WHERE scan_id=?1 AND id=1",rusqlite::params![id,total,files,dirs,integer(if mode=="wide"{count}else{count.div_ceil(64)})?])?;
    c.execute("UPDATE scans SET discovered=?2,status='completed',revision=revision+1,finished=?3,notes_json=?4 WHERE id=?1",rusqlite::params![id,integer(count+1)?,now(),serde_json::to_string(&vec!["Synthetic metadata for scalability and exact-integer QA. No source files exist; file operations fail closed."])?])?;
    service.repo.scan(&id)
}

#[cfg(test)]
mod terminal_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn terminal_notification_bypasses_progress_backpressure_and_is_once() {
        for status in [
            "completed",
            "completed_with_errors",
            "cancelled",
            "failed",
            "interrupted",
        ] {
            let notifications = ScanNotifications::new(Instant::now());
            let job = JobRuntime::new(
                JobEvent::new("job", "scan", "scan", 1),
                Some(notifications.callback.clone()),
            );
            job.update(|event| {
                event.status = "running".into();
                event.discovered_count = "2".into();
            });
            let first = *notifications.first_result_elapsed_ms.lock().unwrap();
            assert!(first.is_some());
            // The first progress event is deliberately never acknowledged.
            job.update(|event| event.discovered_count = "3".into());
            assert!(notifications.terminal.try_recv().is_err());
            job.update(|event| {
                event.status = status.into();
                if status == "failed" {
                    event.error = Some(AppError::new("WORKER_FAILED", "injected fault"));
                }
            });
            // Buffered before the waiter starts; this also covers a scan that
            // terminates before start_scan returns to the probe.
            let terminal = notifications
                .terminal
                .recv_timeout(Duration::from_secs(1))
                .unwrap();
            assert_eq!(terminal.event.status, status);
            assert_eq!(terminal.event.seq, 3);
            assert!(terminal.callback_elapsed_ms >= terminal.event.elapsed_ms as f64);
            assert_eq!(
                *notifications.first_result_elapsed_ms.lock().unwrap(),
                first
            );
            if status == "failed" {
                assert_eq!(terminal.event.error.unwrap().code, "WORKER_FAILED");
            }
            job.update(|event| event.phase = "duplicate terminal update".into());
            assert!(notifications.terminal.try_recv().is_err());
        }
    }

    #[test]
    fn terminal_callback_never_waits_for_a_full_or_disconnected_receiver() {
        let notifications = ScanNotifications::new(Instant::now());
        let callback = notifications.callback.clone();
        let mut event = JobEvent::new("job", "scan", "scan", 1);
        event.status = "cancelled".into();
        callback(event.clone());
        // A second callback must return while the one-slot channel is full.
        let (done, finished) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            callback(event);
            done.send(()).unwrap();
        });
        finished.recv_timeout(Duration::from_secs(1)).unwrap();
        let notifications = ScanNotifications::new(Instant::now());
        let callback = notifications.callback.clone();
        drop(notifications);
        let (done, finished) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            let mut event = JobEvent::new("job", "scan", "scan", 1);
            event.status = "failed".into();
            callback(event);
            done.send(()).unwrap();
        });
        finished.recv_timeout(Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn database_scan_failure_notifies_without_waiting_forever() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("source");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("file"), b"abc").unwrap();
        let service = Service::new(&fixture.path().join("data")).unwrap();
        {
            let c = service.repo.connect().unwrap();
            c.execute_batch("CREATE TRIGGER fail_child_insert BEFORE INSERT ON nodes WHEN NEW.id>1 BEGIN SELECT RAISE(ABORT,'injected scan insert failure'); END;").unwrap();
        }
        let notifications = ScanNotifications::new(Instant::now());
        let scan = service
            .start_scan(
                platform::absolute(&root).unwrap(),
                Some(notifications.callback.clone()),
            )
            .unwrap();
        let terminal = notifications
            .terminal
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert_eq!(terminal.event.status, "failed");
        assert_eq!(
            finished_scan(&service, &scan.scan_id, &terminal.event)
                .unwrap_err()
                .code,
            "DATABASE_ERROR"
        );
        assert_eq!(service.repo.scan(&scan.scan_id).unwrap().status, "failed");
        assert!(notifications.terminal.try_recv().is_err());
    }

    #[test]
    fn runtime_failure_overrides_completed_database_and_final_status_is_reread() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("source");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("file"), b"abc").unwrap();
        let service = Service::new(&fixture.path().join("data")).unwrap();
        let notifications = ScanNotifications::new(Instant::now());
        let scan = service
            .start_scan(
                platform::absolute(&root).unwrap(),
                Some(notifications.callback.clone()),
            )
            .unwrap();
        let mut event = notifications
            .terminal
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .event;
        assert!(notifications.terminal.try_recv().is_err());
        assert_eq!(
            finished_scan(&service, &scan.scan_id, &event)
                .unwrap()
                .status,
            "completed"
        );
        // Model close failure after the completed transaction, including the
        // case where the failure handler could not change that durable row.
        event.status = "failed".into();
        event.error = Some(AppError::new("DATABASE_ERROR", "injected close failure"));
        let error = finished_scan(&service, &scan.scan_id, &event).unwrap_err();
        assert_eq!(error.code, "DATABASE_ERROR");
        assert_eq!(error.message, "injected close failure");
        assert_eq!(
            service.repo.scan(&scan.scan_id).unwrap().status,
            "completed"
        );
        event.error = None;
        assert_eq!(
            finished_scan(&service, &scan.scan_id, &event)
                .unwrap_err()
                .code,
            "SCAN_FAILED"
        );
        event.status = "completed".into();
        service.repo.finish(&scan.scan_id, "failed").unwrap();
        assert_eq!(
            finished_scan(&service, &scan.scan_id, &event)
                .unwrap_err()
                .code,
            "JOB_STATE_INCONSISTENT"
        );
    }
}
