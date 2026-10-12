use crate::{
    export, file_ops,
    model::*,
    platform,
    repository::Repository,
    service::{ProgressCallback, Service},
};
use std::{path::PathBuf, sync::Arc};
use tauri::{ipc::JavaScriptChannelId, Manager, State};
use tauri_plugin_dialog::DialogExt;
type AppState = Arc<Service>;
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || {
        static BUDGET: std::sync::OnceLock<(std::sync::Mutex<usize>, std::sync::Condvar)> =
            std::sync::OnceLock::new();
        let (lock, changed) =
            BUDGET.get_or_init(|| (std::sync::Mutex::new(0), std::sync::Condvar::new()));
        let mut active = lock.lock().unwrap();
        while *active >= 4 {
            active = changed.wait(active).unwrap();
        }
        *active += 1;
        drop(active);
        struct BudgetGuard<'a>(&'a std::sync::Mutex<usize>, &'a std::sync::Condvar);
        impl Drop for BudgetGuard<'_> {
            fn drop(&mut self) {
                *self.0.lock().unwrap() -= 1;
                self.1.notify_one();
            }
        }
        let _budget = BudgetGuard(lock, changed);
        work()
    })
    .await
    .map_err(|e| AppError::new("WORKER_FAILED", e.to_string()))?
}
fn progress(
    channel: Option<JavaScriptChannelId>,
    webview: tauri::Webview,
) -> Option<ProgressCallback> {
    channel.map(|id| {
        let c = id.channel_on::<_, JobEvent>(webview);
        Arc::new(move |e| {
            let _ = c.send(e);
        }) as ProgressCallback
    })
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartRequest {
    source_ref: Option<String>,
    path: Option<String>,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchOptions {
    fixture: bool,
    scan_path: Option<String>,
}
fn arg_value(name: &str) -> Option<String> {
    let args = std::env::args().collect::<Vec<_>>();
    for (i, arg) in args.iter().enumerate() {
        if let Some(v) = arg.strip_prefix(&format!("{name}=")) {
            return Some(v.into());
        }
        if arg == name {
            return args.get(i + 1).cloned();
        }
    }
    None
}
#[tauri::command]
fn get_launch_options() -> LaunchOptions {
    LaunchOptions {
        fixture: arg_value("--fixture").as_deref() == Some("1")
            && (cfg!(debug_assertions) || arg_value("--qa-data-dir").is_some()),
        scan_path: arg_value("--scan-path"),
    }
}
#[tauri::command]
async fn choose_scan_source(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<Source>> {
    let service = state.inner().clone();
    blocking(move || {
        match app
            .dialog()
            .file()
            .set_title("选择扫描位置")
            .blocking_pick_folder()
        {
            Some(p) => {
                Ok(Some(service.register_source(p.into_path().map_err(
                    |e| AppError::new("INVALID_PATH", e.to_string()),
                )?)?))
            }
            None => Ok(None),
        }
    })
    .await
}
#[tauri::command]
async fn choose_destination(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<Source>> {
    let service = state.inner().clone();
    blocking(move || {
        match app
            .dialog()
            .file()
            .set_title("选择目标目录")
            .blocking_pick_folder()
        {
            Some(p) => {
                Ok(Some(service.register_source(p.into_path().map_err(
                    |e| AppError::new("INVALID_PATH", e.to_string()),
                )?)?))
            }
            None => Ok(None),
        }
    })
    .await
}
#[tauri::command]
async fn start_scan(
    state: State<'_, AppState>,
    request: StartRequest,
    on_progress: Option<JavaScriptChannelId>,
    webview: tauri::Webview,
) -> Result<Scan> {
    let service = state.inner().clone();
    blocking(move || {
        let path =
            service.resolve_source(request.source_ref.as_deref(), request.path.as_deref())?;
        service.start_scan(path, progress(on_progress, webview))
    })
    .await
}
#[tauri::command]
fn cancel_scan(state: State<'_, AppState>, scan_id: String) -> Result<()> {
    state.cancel_job(&scan_id)
}
#[tauri::command]
async fn get_scan(state: State<'_, AppState>, scan_id: String) -> Result<Scan> {
    let s = state.inner().clone();
    blocking(move || s.repo.scan(&scan_id)).await
}
#[tauri::command]
async fn load_scan(state: State<'_, AppState>, scan_id: String) -> Result<Scan> {
    let s = state.inner().clone();
    blocking(move || {
        let _writer = s.writer.lock().unwrap();
        s.repo.load_history(&scan_id)
    })
    .await
}
#[tauri::command]
async fn get_recent_scans(state: State<'_, AppState>) -> Result<Vec<Scan>> {
    let s = state.inner().clone();
    blocking(move || s.repo.recent()).await
}
#[tauri::command]
async fn get_node(
    state: State<'_, AppState>,
    scan_id: String,
    node_id: String,
    revision: u32,
) -> Result<Node> {
    let s = state.inner().clone();
    blocking(move || Ok(s.repo.node(&scan_id, &node_id, Some(revision))?.node)).await
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Measurement {
    node: Node,
    revision: u32,
}
#[tauri::command]
async fn measure_node(
    state: State<'_, AppState>,
    scan_id: String,
    node_id: String,
    revision: u32,
) -> Result<Measurement> {
    let s = state.inner().clone();
    blocking(move || {
        let (node, revision) = s.measure_node(&scan_id, &node_id, revision)?;
        Ok(Measurement { node, revision })
    })
    .await
}
#[tauri::command]
async fn get_children(state: State<'_, AppState>, request: PageRequest) -> Result<NodePage> {
    let s = state.inner().clone();
    blocking(move || s.repo.page(&request, false)).await
}
#[tauri::command]
async fn get_node_page(
    state: State<'_, AppState>,
    scan_id: String,
    node_id: String,
    revision: u32,
    sort: Option<String>,
    page_size: Option<u32>,
) -> Result<NodePage> {
    let s = state.inner().clone();
    blocking(move || {
        s.repo.node_page(
            &scan_id,
            &node_id,
            revision,
            sort.as_deref().unwrap_or("size_desc"),
            page_size.unwrap_or(200),
        )
    })
    .await
}
#[tauri::command]
async fn search_nodes(state: State<'_, AppState>, request: PageRequest) -> Result<NodePage> {
    let s = state.inner().clone();
    blocking(move || s.repo.page(&request, true)).await
}
#[tauri::command]
async fn get_ancestors(
    state: State<'_, AppState>,
    scan_id: String,
    node_id: String,
    revision: u32,
) -> Result<Vec<Node>> {
    let s = state.inner().clone();
    blocking(move || s.repo.ancestors(&scan_id, &node_id, revision)).await
}
#[tauri::command]
async fn get_chart_data(
    state: State<'_, AppState>,
    scan_id: String,
    scope_id: String,
    revision: u32,
    depth: Option<u32>,
    node_budget: Option<u32>,
) -> Result<ChartData> {
    let s = state.inner().clone();
    blocking(move || {
        s.repo.chart(
            &scan_id,
            &scope_id,
            revision,
            depth.unwrap_or(2),
            node_budget.unwrap_or(256),
        )
    })
    .await
}
#[tauri::command]
async fn get_selection_summary(
    state: State<'_, AppState>,
    scan_id: String,
    node_ids: Vec<String>,
    revision: u32,
) -> Result<SelectionSummary> {
    let s = state.inner().clone();
    blocking(move || s.repo.selection(&scan_id, &node_ids, revision)).await
}
#[tauri::command]
async fn get_scan_errors(
    state: State<'_, AppState>,
    scan_id: String,
    cursor: Option<String>,
    page_size: Option<u32>,
) -> Result<ErrorPage> {
    let s = state.inner().clone();
    blocking(move || {
        s.repo
            .errors(&scan_id, cursor.as_deref(), page_size.unwrap_or(100))
    })
    .await
}
#[tauri::command]
async fn plan_operation(
    state: State<'_, AppState>,
    request: OperationRequest,
) -> Result<OperationPlan> {
    let s = state.inner().clone();
    blocking(move || file_ops::plan(&s, request)).await
}
#[tauri::command]
async fn execute_operation(
    state: State<'_, AppState>,
    plan_id: String,
    on_progress: Option<JavaScriptChannelId>,
    webview: tauri::Webview,
) -> Result<Job> {
    let s = state.inner().clone();
    blocking(move || file_ops::execute(&s, &plan_id, progress(on_progress, webview))).await
}
#[tauri::command]
fn cancel_operation(state: State<'_, AppState>, job_id: String) -> Result<()> {
    state.cancel_job(&job_id)
}
#[tauri::command]
fn get_job(state: State<'_, AppState>, job_id: String) -> Result<JobEvent> {
    Ok(state.job(&job_id)?.snapshot())
}
#[tauri::command]
fn ack_job(state: State<'_, AppState>, job_id: String, seq: u64) -> Result<()> {
    state.job(&job_id)?.acknowledge(seq)
}
#[tauri::command]
async fn open_node(
    state: State<'_, AppState>,
    scan_id: String,
    node_id: String,
    action: String,
) -> Result<()> {
    let s = state.inner().clone();
    blocking(move || {
        let node = s.repo.node(&scan_id, &node_id, None)?;
        let current = platform::metadata(&node.path)?;
        if node.fingerprint.identity.is_some() && current.identity != node.fingerprint.identity {
            return Err(AppError::new(
                "SOURCE_CHANGED",
                "扫描后项目身份已变化，请刷新",
            ));
        }
        file_ops::open_node(&node.path, &action)
    })
    .await
}
#[tauri::command]
async fn copy_paths(
    state: State<'_, AppState>,
    window: tauri::Window,
    scan_id: String,
    node_ids: Vec<String>,
) -> Result<String> {
    let s = state.inner().clone();
    #[cfg(windows)]
    let owner = window
        .hwnd()
        .map_err(|e| AppError::new("CLIPBOARD_ERROR", e.to_string()))?
        .0 as isize;
    blocking(move || {
        let c = s.repo.connect()?;
        let (nodes, _) = Repository::normalize_conn(&c, &scan_id, &node_ids)?;
        let paths = nodes
            .iter()
            .map(|n| n.node.display_path.as_str())
            .collect::<Vec<_>>()
            .join("\r\n");
        if paths.len() > 2 * 1024 * 1024 {
            return Err(AppError::new(
                "CLIPBOARD_LIMIT",
                "所选路径超过剪贴板文本预算",
            ));
        }
        #[cfg(windows)]
        platform::copy_raw_paths(
            &nodes.iter().map(|n| n.path.clone()).collect::<Vec<_>>(),
            owner,
        )?;
        #[cfg(not(windows))]
        {
            let mut clipboard = arboard::Clipboard::new()
                .map_err(|e| AppError::new("CLIPBOARD_ERROR", e.to_string()))?;
            clipboard
                .set_text(paths.clone())
                .map_err(|e| AppError::new("CLIPBOARD_ERROR", e.to_string()))?;
        }
        Ok(paths)
    })
    .await
}
#[tauri::command]
async fn get_export_preview(
    state: State<'_, AppState>,
    request: ExportRequest,
) -> Result<ExportPreview> {
    let s = state.inner().clone();
    blocking(move || export::preview(&s, &request)).await
}
#[tauri::command]
async fn start_export(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ExportRequest,
    on_progress: Option<JavaScriptChannelId>,
    webview: tauri::Webview,
) -> Result<Option<Job>> {
    let s = state.inner().clone();
    blocking(move || {
        let preview = export::preview(&s, &request)?;
        let target = if let Some(path) = &request.target_path {
            PathBuf::from(path)
        } else {
            let path = app
                .dialog()
                .file()
                .set_title("保存扫描报告")
                .set_file_name(&preview.suggested_name)
                .add_filter(request.format.to_uppercase(), &[request.format.as_str()])
                .blocking_save_file();
            let Some(path) = path else { return Ok(None) };
            path.into_path()
                .map_err(|e| AppError::new("INVALID_PATH", e.to_string()))?
        };
        let job =
            export::start_with_target(&s, request.clone(), target, progress(on_progress, webview))?;
        let mut settings = s.read_settings();
        settings.last_export_format = request.format;
        let _ = s.save_settings(settings);
        Ok(Some(job))
    })
    .await
}
#[tauri::command]
fn cancel_export(state: State<'_, AppState>, job_id: String) -> Result<()> {
    state.cancel_job(&job_id)
}
#[tauri::command]
fn read_settings(state: State<'_, AppState>) -> Settings {
    state.read_settings()
}
#[tauri::command]
async fn update_settings(state: State<'_, AppState>, settings: Settings) -> Result<Settings> {
    let s = state.inner().clone();
    blocking(move || s.save_settings(settings)).await
}
#[tauri::command]
async fn refresh_scope(
    state: State<'_, AppState>,
    scan_id: String,
    scope_id: String,
    on_progress: Option<JavaScriptChannelId>,
    webview: tauri::Webview,
) -> Result<Scan> {
    let s = state.inner().clone();
    blocking(move || s.refresh(scan_id, scope_id, progress(on_progress, webview))).await
}
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data = if let Some(path) = arg_value("--qa-data-dir") {
                let p = PathBuf::from(path);
                if !p.is_absolute() {
                    return Err("--qa-data-dir must be absolute".into());
                }
                p
            } else {
                app.path().app_data_dir()?
            };
            app.manage(Service::new(&data)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_launch_options,
            choose_scan_source,
            choose_destination,
            start_scan,
            cancel_scan,
            get_scan,
            load_scan,
            get_recent_scans,
            get_node,
            measure_node,
            get_children,
            get_node_page,
            search_nodes,
            get_ancestors,
            get_chart_data,
            get_selection_summary,
            get_scan_errors,
            plan_operation,
            execute_operation,
            cancel_operation,
            get_job,
            ack_job,
            open_node,
            copy_paths,
            get_export_preview,
            start_export,
            cancel_export,
            read_settings,
            update_settings,
            refresh_scope
        ])
        .run(tauri::generate_context!())
        .expect("Spacefold desktop runtime");
}
