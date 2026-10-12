use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::SystemTime};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_code: Option<i32>,
}
pub type Result<T> = std::result::Result<T, AppError>;
impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
            node_id: None,
            os_code: None,
        }
    }
    pub fn io(error: std::io::Error) -> Self {
        let code = match error.kind() {
            std::io::ErrorKind::NotFound => "NOT_FOUND",
            std::io::ErrorKind::PermissionDenied => "ACCESS_DENIED",
            std::io::ErrorKind::AlreadyExists => "DESTINATION_CONFLICT",
            _ => "IO_ERROR",
        };
        Self {
            code: code.into(),
            message: error.to_string(),
            retryable: true,
            node_id: None,
            os_code: error.raw_os_error(),
        }
    }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for AppError {}
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::io(e)
    }
}
impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        Self::new("DATABASE_ERROR", e.to_string())
    }
}
impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::new("JSON_ERROR", e.to_string())
    }
}
impl From<rust_xlsxwriter::XlsxError> for AppError {
    fn from(e: rust_xlsxwriter::XlsxError) -> Self {
        Self::new("FORMAT_LIMIT", format!("{e}；请改用 JSON 保存完整数据"))
    }
}
impl From<csv::Error> for AppError {
    fn from(e: csv::Error) -> Self {
        Self::new("EXPORT_ERROR", e.to_string())
    }
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub fn time_string(t: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub fn integer(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| AppError::new("SIZE_OUT_OF_RANGE", "字节数超出 SQLite 支持的精确整数范围"))
}
pub fn parse_id(value: &str) -> Result<i64> {
    value
        .parse::<i64>()
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| AppError::new("INVALID_NODE_ID", "节点标识无效"))
}
pub fn parse_bytes(value: &str) -> Result<i64> {
    value
        .parse::<u64>()
        .ok()
        .and_then(|v| i64::try_from(v).ok())
        .ok_or_else(|| AppError::new("SIZE_OUT_OF_RANGE", "请输入有效非负字节整数"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub source_ref: String,
    pub display_path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSpace {
    pub total_bytes: String,
    pub free_bytes: String,
    pub available_bytes: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    pub scan_id: String,
    pub root_id: String,
    pub source: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub revision: u32,
    pub discovered_count: String,
    pub logical_total_bytes: String,
    pub error_count: String,
    pub incomplete: bool,
    pub stale: bool,
    pub disk_space: Option<DiskSpace>,
    pub coverage_notes: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub node_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub display_path: String,
    pub kind: String,
    pub extension: String,
    pub depth: u32,
    pub logical_bytes: String,
    pub logical_total_bytes: String,
    pub allocated_bytes: Option<String>,
    pub allocated_known_total_bytes: String,
    pub allocated_unknown_count: String,
    pub file_count: String,
    pub directory_count: String,
    pub modified_at: Option<String>,
    pub identity: Option<String>,
    pub link_count: Option<String>,
    pub link_type: Option<String>,
    pub link_target: Option<String>,
    pub cloud_state: Option<String>,
    pub allocation_method: Option<String>,
    pub error_code: Option<String>,
    pub incomplete: bool,
    pub stale: bool,
    pub children_count: String,
}
#[derive(Debug, Clone)]
pub struct StoredNode {
    pub node: Node,
    pub path: PathBuf,
    pub fingerprint: Fingerprint,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Fingerprint {
    pub identity: Option<String>,
    pub kind: String,
    pub logical: u64,
    pub modified: Option<String>,
    pub attributes: u32,
    pub reparse_tag: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub keyword: Option<String>,
    pub kind: Option<String>,
    pub extension: Option<String>,
    pub min_bytes: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRequest {
    pub scan_id: String,
    pub parent_id: Option<String>,
    pub revision: u32,
    pub sort: Option<String>,
    pub filter: Option<Filter>,
    pub cursor: Option<String>,
    pub page_size: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePage {
    pub revision: u32,
    pub nodes: Vec<Node>,
    pub next_cursor: Option<String>,
    pub total_count: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartNode {
    pub node: Node,
    pub children: Vec<ChartNode>,
    pub other_count: String,
    pub other_bytes: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartData {
    pub revision: u32,
    pub root: ChartNode,
    pub truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSummary {
    pub nodes: Vec<Node>,
    pub normalized_ids: Vec<String>,
    pub covered_ids: Vec<String>,
    pub logical_total_bytes: String,
    pub allocated_known_total_bytes: String,
    pub allocated_unknown_count: String,
    pub count: String,
    pub can_open: bool,
    pub can_rename: bool,
    pub can_move: bool,
    pub can_trash: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanError {
    pub node_id: Option<String>,
    pub display_path: String,
    pub stage: String,
    pub error: AppError,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPage {
    pub errors: Vec<ScanError>,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub schema_version: u32,
    pub accent: String,
    pub theme: String,
    pub size_unit: String,
    pub default_view: String,
    pub recent_locations: Vec<String>,
    pub last_export_format: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            accent: "purple".into(),
            theme: "system".into(),
            size_unit: "decimal".into(),
            default_view: "tree".into(),
            recent_locations: vec![],
            last_export_format: "csv".into(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || !["purple", "blue", "teal", "amber", "rose"].contains(&self.accent.as_str())
            || !["system", "light", "dark"].contains(&self.theme.as_str())
            || !["decimal", "binary"].contains(&self.size_unit.as_str())
            || !["tree", "map", "lens"].contains(&self.default_view.as_str())
            || !["csv", "xlsx", "json", "html"].contains(&self.last_export_format.as_str())
            || self.recent_locations.len() > 20
        {
            return Err(AppError::new("INVALID_SETTINGS", "设置包含无效值"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRequest {
    pub scan_id: String,
    pub node_ids: Vec<String>,
    pub expected_revision: u32,
    pub kind: String,
    pub destination_ref: Option<String>,
    pub destination_path: Option<String>,
    pub new_name: Option<String>,
    pub conflict_policy: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub node_id: String,
    pub name: String,
    pub display_path: String,
    pub destination: Option<String>,
    pub error: Option<AppError>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationPlan {
    pub plan_id: String,
    pub scan_id: String,
    pub revision: u32,
    pub kind: String,
    pub items: Vec<PlanItem>,
    pub logical_total_bytes: String,
    pub expires_at: String,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    pub node_id: String,
    pub display_path: String,
    pub status: String,
    pub destination: Option<String>,
    pub error: Option<AppError>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub job_id: String,
    pub scan_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEvent {
    pub job_id: String,
    pub scan_id: String,
    pub revision: u32,
    pub seq: u64,
    pub r#type: String,
    pub status: String,
    pub discovered_count: String,
    pub logical_total_bytes: String,
    pub error_count: String,
    pub written_count: String,
    pub elapsed_ms: u64,
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<OperationResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AppError>,
}
impl JobEvent {
    pub fn new(job_id: &str, scan_id: &str, kind: &str, revision: u32) -> Self {
        Self {
            job_id: job_id.into(),
            scan_id: scan_id.into(),
            revision,
            seq: 0,
            r#type: kind.into(),
            status: "queued".into(),
            discovered_count: "0".into(),
            logical_total_bytes: "0".into(),
            error_count: "0".into(),
            written_count: "0".into(),
            elapsed_ms: 0,
            phase: "等待开始".into(),
            results: None,
            output_path: None,
            error: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub scan_id: String,
    pub revision: u32,
    pub scope: String,
    pub scope_id: String,
    pub selected_ids: Vec<String>,
    pub recursive: bool,
    pub fields: Vec<String>,
    pub format: String,
    pub target_path: Option<String>,
    pub html_row_budget: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPreview {
    pub record_count: String,
    pub logical_total_bytes: String,
    pub sample: Vec<serde_json::Map<String, serde_json::Value>>,
    pub suggested_name: String,
    pub incomplete: bool,
}
