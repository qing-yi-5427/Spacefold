/** Shared production IPC contract. All byte counts and node/count IDs are decimal strings. */
export type Bytes = string;
export type ScanStatus = 'queued'|'running'|'cancelling'|'cancelled'|'completed'|'completed_with_errors'|'failed'|'interrupted';
export type NodeKind = 'directory'|'file'|'symlink'|'junction'|'mount_point'|'other';
export interface AppError { code: string; message: string; retryable: boolean; nodeId?: string; osCode?: number; }
export interface Source { sourceRef: string; displayPath: string; }
export interface DiskSpace { totalBytes: Bytes; freeBytes: Bytes; availableBytes: Bytes; }
export interface Scan { scanId: string; rootId: string; source: string; startedAt: string; finishedAt: string|null; status: ScanStatus; revision: number; discoveredCount: string; logicalTotalBytes: Bytes; errorCount: string; incomplete: boolean; stale: boolean; diskSpace: DiskSpace|null; coverageNotes: string[]; }
export interface Node { nodeId: string; parentId: string|null; name: string; displayPath: string; kind: NodeKind; extension: string; depth: number; logicalBytes: Bytes; logicalTotalBytes: Bytes; allocatedBytes: Bytes|null; allocatedKnownTotalBytes: Bytes; allocatedUnknownCount: string; fileCount: string; directoryCount: string; modifiedAt: string|null; identity: string|null; linkCount: string|null; linkType: string|null; linkTarget: string|null; cloudState: string|null; allocationMethod: string|null; errorCode: string|null; incomplete: boolean; stale: boolean; childrenCount: string; }
export interface Filter { keyword?: string; kind?: 'all'|NodeKind; extension?: string; minBytes?: Bytes; }
export interface PageRequest { scanId: string; parentId?: string; revision: number; sort?: 'size_desc'|'size_asc'|'name_asc'|'name_desc'|'modified_desc'; filter?: Filter; cursor?: string|null; pageSize?: number; }
export interface NodePage { revision: number; nodes: Node[]; nextCursor: string|null; totalCount: string; pageCursor?:string|null; }
export interface ChartNode { node: Node; children: ChartNode[]; otherCount: string; otherBytes: Bytes; }
export interface ChartData { revision: number; root: ChartNode; truncated: boolean; }
export interface SelectionSummary { nodes: Node[]; normalizedIds: string[]; coveredIds: string[]; logicalTotalBytes: Bytes; allocatedKnownTotalBytes: Bytes; allocatedUnknownCount: string; count: string; canOpen: boolean; canRename: boolean; canMove: boolean; canTrash: boolean; }
export interface ViewSnapshotRequest { scanId:string;scopeId:string;expandedIds:string[];selectedIds:string[];primaryId?:string|null;sort?:PageRequest['sort'];pageAnchors?:{parentId:string;firstId:string}[]; }
export interface ViewSnapshot { scan:Scan;scopeId:string;primaryId:string|null;expandedIds:string[];selectedIds:string[];nodes:Node[];pages:{parentId:string;page:NodePage}[];chart:ChartData;summary:SelectionSummary;details:{nodeId:string;nodes:Node[]}[]; }
export type OperationKind = 'copy'|'move'|'rename'|'trash';
export type ConflictPolicy = 'skip'|'keep_both'|'replace';
export interface OperationRequest { scanId: string; nodeIds: string[]; expectedRevision: number; kind: OperationKind; destinationRef?: string; destinationPath?: string; newName?: string; conflictPolicy?: ConflictPolicy; }
export interface PlanItem { nodeId: string; name: string; displayPath: string; destination: string|null; error: AppError|null; }
export interface OperationPlan { planId: string; scanId: string; revision: number; kind: OperationKind; items: PlanItem[]; logicalTotalBytes: Bytes; expiresAt: string; warnings: string[]; }
export interface OperationResult { nodeId: string; displayPath: string; status: 'success'|'failed'|'skipped'|'cancelled'; destination: string|null; error: AppError|null; }
export interface JobEvent { jobId: string; scanId: string; revision: number; seq: number; type: 'scan'|'operation'|'export'; status: string; discoveredCount: string; logicalTotalBytes: Bytes; errorCount: string; writtenCount: string; elapsedMs: number; phase: string; results?: OperationResult[]; outputPath?: string; error?: AppError; }
export interface Job { jobId: string; scanId: string; }
export type ExportFormat = 'csv'|'xlsx'|'json'|'html';
export type ExportField = 'name'|'path'|'logical_bytes'|'kind'|'parent_path'|'depth'|'own_logical_bytes'|'allocated_bytes'|'modified_at'|'node_id'|'parent_id'|'measurement_status'|'link_type'|'error_code';
export interface ExportRequest { scanId: string; revision: number; scope: 'all'|'current'|'selected'; scopeId: string; selectedIds: string[]; recursive: boolean; fields: ExportField[]; format: ExportFormat; targetPath?: string; htmlRowBudget?: number; }
export interface ExportPreview { recordCount: string; logicalTotalBytes: Bytes; sample: Record<string,string|null>[]; suggestedName: string; incomplete: boolean; }
export interface Settings { schemaVersion: number; accent: 'purple'|'blue'|'teal'|'amber'|'rose'; theme: 'system'|'light'|'dark'; sizeUnit: 'decimal'|'binary'; defaultView: 'tree'|'map'|'lens'; recentLocations: string[]; lastExportFormat: ExportFormat; }
export interface ScanError { nodeId: string|null; displayPath: string; stage: string; error: AppError; }
/** invoke uses `{request: ...}` for typed requests; scalar commands use exact camelCase args below.
 * choose_scan_source() -> Source|null; choose_destination() -> Source|null
 * start_scan({request:{sourceRef?:string,path?:string},onProgress?:Channel<JobEvent>}) -> Scan
 * cancel_scan({scanId}) -> void; get_scan({scanId}) / load_scan({scanId}) -> Scan
 * get_recent_scans() -> Scan[]; get_node({scanId,nodeId,revision}) -> Node
 * measure_node({scanId,nodeId,revision}) -> {node:Node,revision:number} (supplement allocation, identity and links; committed revision)
 * get_children({request:PageRequest}) / search_nodes({request:PageRequest}) -> NodePage
 * get_node_page({scanId,nodeId,revision,sort?,pageSize?}) -> NodePage (ordered sibling page starting with target, pageCursor replays that page)
 * get_ancestors({scanId,nodeId,revision}) -> Node[] (root through parent)
 * get_chart_data({scanId,scopeId,revision,depth?:number,nodeBudget?:number}) -> ChartData
 * get_selection_summary({scanId,nodeIds,revision}) -> SelectionSummary
 * get_view_snapshot({request:ViewSnapshotRequest}) -> ViewSnapshot (one committed read transaction; operation revision checks remain separate)
 * get_scan_errors({scanId,cursor?:string,pageSize?:number}) -> {errors:ScanError[],nextCursor:string|null}
 * plan_operation({request:OperationRequest}) -> OperationPlan
 * execute_operation({planId,onProgress?:Channel<JobEvent>}) -> Job
 * cancel_operation({jobId}) -> void; get_job({jobId}) -> JobEvent
 * ack_job({jobId,seq}) -> void (acknowledge each processed Channel event; unacknowledged events are coalesced)
 * open_node({scanId,nodeId,action:'open'|'reveal'|'properties'}) -> void
 * copy_paths({scanId,nodeIds}) -> string (also writes system clipboard)
 * get_export_preview({request:ExportRequest}) -> ExportPreview
 * start_export({request:ExportRequest,onProgress?:Channel<JobEvent>}) -> Job|null (native save cancelled)
 * cancel_export({jobId}) -> void
 * read_settings() -> Settings; update_settings({settings:Settings}) -> Settings
 * refresh_scope({scanId,scopeId,onProgress?:Channel<JobEvent>}) -> Scan
 */
