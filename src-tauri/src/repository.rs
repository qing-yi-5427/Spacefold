use crate::{model::*, platform};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

pub const NODE_COLUMNS:&str="id,parent_id,path,payload,logical_total,allocated_total,allocated_unknown,file_count,dir_count,children_count,incomplete,(stale OR (SELECT history_stale FROM scans WHERE id=nodes.scan_id)),error_code,depth,name,kind,extension,modified,search_path";
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
pub struct Repository {
    pub path: PathBuf,
    pub data_dir: PathBuf,
    id_allocator: std::sync::Mutex<()>,
}
impl Repository {
    pub fn new(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let data_dir = std::fs::canonicalize(data_dir)?;
        let repo = Self {
            path: data_dir.join("spacefold.sqlite3"),
            data_dir,
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
    CREATE INDEX IF NOT EXISTS nodes_parent_name_v2 ON nodes(scan_id,parent_id,name COLLATE NOCASE,id);
    CREATE TABLE IF NOT EXISTS pending_dirs(scan_id TEXT NOT NULL,id INTEGER NOT NULL,path BLOB NOT NULL,depth INTEGER NOT NULL,PRIMARY KEY(scan_id,id));
    CREATE TABLE IF NOT EXISTS scan_errors(id INTEGER PRIMARY KEY AUTOINCREMENT,scan_id TEXT NOT NULL,payload TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS operations(id TEXT PRIMARY KEY,scan_id TEXT NOT NULL,kind TEXT NOT NULL,started TEXT NOT NULL,finished TEXT,payload TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS scan_sequence(next_id INTEGER NOT NULL CHECK(typeof(next_id)='integer' AND next_id>0));INSERT INTO scan_sequence SELECT 1 WHERE NOT EXISTS(SELECT 1 FROM scan_sequence);
    UPDATE scans SET status='interrupted',finished=strftime('%Y-%m-%dT%H:%M:%fZ','now'),stale=1 WHERE status IN('running','queued','cancelling');
    DELETE FROM pending_dirs;")?;
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
        c.execute_batch("CREATE INDEX IF NOT EXISTS nodes_parent_size_v3 ON nodes(scan_id,parent_id,sort_size,name COLLATE NOCASE,id); CREATE INDEX IF NOT EXISTS nodes_incomplete ON nodes(scan_id,id) WHERE incomplete=1; CREATE INDEX IF NOT EXISTS nodes_special_kind_v2 ON nodes(scan_id,kind,parent_id,sort_size,name COLLATE NOCASE,id,logical_total,extension,modified) WHERE kind<>'file'; DROP INDEX IF EXISTS nodes_parent_size_asc_v2; DROP INDEX IF EXISTS nodes_kind; DROP INDEX IF EXISTS nodes_special_kind; DROP INDEX IF EXISTS nodes_parent_size; DROP INDEX IF EXISTS nodes_parent_name; DROP INDEX IF EXISTS nodes_parent_size_v2;")?;
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
        let c = Connection::open(&self.path)?;
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
            "普通文件已分配大小与身份按详情/操作计划查询；未取得值为未知，已知占用只表示已测部分。",
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
        let (nodes, covered) = Self::normalize_conn(&tx, scan_id, ids)?;
        let mut bytes = 0i64;
        let mut alloc = 0i64;
        let mut unknown = 0i64;
        for n in &nodes {
            bytes = checked_add(bytes, parse_bytes(&n.node.logical_total_bytes)?)?;
            alloc = checked_add(alloc, parse_bytes(&n.node.allocated_known_total_bytes)?)?;
            unknown = checked_add(unknown, parse_bytes(&n.node.allocated_unknown_count)?)?
        }
        let mutable = !nodes.is_empty() && nodes.iter().all(|n| n.node.parent_id.is_some());
        let single = nodes.len() == 1;
        Ok(SelectionSummary {
            normalized_ids: nodes.iter().map(|n| n.node.node_id.clone()).collect(),
            covered_ids: covered,
            count: nodes.len().to_string(),
            logical_total_bytes: bytes.to_string(),
            allocated_known_total_bytes: alloc.to_string(),
            allocated_unknown_count: unknown.to_string(),
            can_open: single,
            can_rename: single && mutable,
            can_move: mutable,
            can_trash: mutable,
            nodes: nodes.into_iter().map(|n| n.node).collect(),
        })
    }
    pub fn page(&self, r: &PageRequest, search: bool) -> Result<NodePage> {
        use rusqlite::types::Value;
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Cursor {
            key: String,
            id: i64,
            name: String,
            size: i64,
            modified: String,
        }
        let c = self.connect()?;
        let tx = c.unchecked_transaction()?;
        Self::check_revision(&tx, &r.scan_id, r.revision)?;
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
        if cursor.as_ref().is_some_and(|c| c.key != key) {
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
        Self::check_revision(&tx, scan_id, revision)?;
        let target = Self::node_conn(&tx, scan_id, parse_id(node_id)?)?;
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
        self.page(
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
        Self::check_revision(&tx, scan_id, revision)?;
        let mut left = budget.clamp(2, 2000);
        let mut truncated = false;
        let root = chart_node(
            &tx,
            scan_id,
            parse_id(scope_id)?,
            depth.clamp(1, 4),
            &mut left,
            &mut truncated,
        )?;
        Ok(ChartData {
            revision,
            root,
            truncated,
        })
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
        let connection = self.connect()?;
        let c = connection.unchecked_transaction()?;
        c.execute(
            "UPDATE scans SET status=?2,finished=?3,revision=revision+1 WHERE id=?1",
            params![scan_id, status, now()],
        )?;
        if matches!(status, "completed" | "completed_with_errors") {
            c.execute("UPDATE nodes INDEXED BY nodes_incomplete SET incomplete=CASE WHEN error_code IS NULL THEN 0 ELSE 1 END WHERE scan_id=?1 AND incomplete=1",[scan_id])?;
            if status == "completed_with_errors" {
                c.execute("WITH RECURSIVE affected(id) AS (SELECT id FROM nodes WHERE scan_id=?1 AND error_code IS NOT NULL UNION SELECT n.parent_id FROM affected a CROSS JOIN nodes n ON n.scan_id=?1 AND n.id=a.id WHERE n.parent_id IS NOT NULL) UPDATE nodes SET incomplete=1 WHERE scan_id=?1 AND id IN affected",[scan_id])?;
            }
        }
        c.execute("DELETE FROM pending_dirs WHERE scan_id=?1", [scan_id])?;
        let result = Self::scan_conn(&c, scan_id)?;
        c.commit()?;
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
    pub timings: Option<InsertTimings>,
}
impl<'a> NodeInserter<'a> {
    pub fn new(tx: &'a Transaction<'_>, profile: bool) -> Result<Self> {
        Ok(Self {
            insert: tx.prepare_cached("INSERT INTO nodes(scan_id,id,parent_id,path,payload,name,kind,extension,depth,modified,logical_total,allocated_total,allocated_unknown,file_count,dir_count,error_code,search_path,incomplete) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,0,?15,?16,?17)")?,
            pending: tx.prepare_cached("INSERT INTO pending_dirs(scan_id,id,path,depth) VALUES(?1,?2,?3,?4)")?,
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
            m.recurse || error.is_some()
        ])?;
        if let (Some(timings), Some(started)) = (&mut self.timings, started) {
            timings.sql_ms += started.elapsed().as_secs_f64() * 1000.0;
        }
        if m.recurse {
            let started = self.timings.as_ref().map(|_| std::time::Instant::now());
            self.pending
                .execute(params![scan_id, id, encoded.raw_path, depth])?;
            if let (Some(timings), Some(started)) = (&mut self.timings, started) {
                timings.pending_ms += started.elapsed().as_secs_f64() * 1000.0;
            }
        }
        Ok(())
    }
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
) -> Result<ChartNode> {
    let node = Repository::node_conn(c, scan_id, id)?.node;
    *left = left.saturating_sub(1);
    let mut out = ChartNode {
        node,
        children: vec![],
        other_count: "0".into(),
        other_bytes: "0".into(),
    };
    if depth == 0 || *left == 0 {
        out.other_count = out.node.children_count.clone();
        out.other_bytes = out.node.logical_total_bytes.clone();
        if *left == 0 && out.other_count != "0" {
            *truncated = true;
        }
        return Ok(out);
    }
    let limit = (*left).min(200);
    let sql=format!("SELECT {NODE_COLUMNS} FROM nodes WHERE scan_id=?1 AND parent_id=?2 ORDER BY sort_size,name COLLATE NOCASE,id LIMIT ?3");
    let mut stmt = c.prepare(&sql)?;
    let direct = stmt
        .query_map(params![scan_id, id, limit], read_node)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut known = 0i64;
    let mut count = 0u64;
    for child in direct {
        if *left == 0 {
            break;
        }
        known = checked_add(known, parse_bytes(&child.node.logical_total_bytes)?)?;
        count += 1;
        out.children.push(chart_node(
            c,
            scan_id,
            parse_id(&child.node.node_id)?,
            depth - 1,
            left,
            truncated,
        )?);
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
