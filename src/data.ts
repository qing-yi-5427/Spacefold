import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import type { Scan, Node, NodePage, PageRequest, ChartData, ChartNode, SelectionSummary, Settings, Source, OperationRequest, OperationPlan, Job, JobEvent, ExportRequest, ExportPreview, ScanError } from './contracts';
export type View = 'map'|'tree'|'lens';
export const defaults: Settings = {schemaVersion:1,accent:'purple',theme:'system',sizeUnit:'decimal',defaultView:'tree',recentLocations:[],lastExportFormat:'csv'};
export interface DataSource {
  fixture: boolean;
  settings(): Promise<Settings>; saveSettings(settings:Settings):Promise<Settings>;
  recent():Promise<Scan[]>; load(scanId:string):Promise<Scan>; scan(scanId:string):Promise<Scan>;
  choose():Promise<Source|null>; destination():Promise<Source|null>;
  start(source:Source,callback:(event:JobEvent)=>void):Promise<Scan>; cancelScan(scanId:string):Promise<void>;
  startPath(path:string,callback:(event:JobEvent)=>void):Promise<Scan>;
  refresh(scanId:string,scopeId:string,callback:(event:JobEvent)=>void):Promise<Scan>;
  node(scanId:string,nodeId:string,revision:number):Promise<Node>;
  measure(scanId:string,nodeId:string,revision:number):Promise<{node:Node;revision:number}>;
  children(request:PageRequest):Promise<NodePage>; search(request:PageRequest):Promise<NodePage>;
  nodePage(scanId:string,nodeId:string,revision:number,sort:PageRequest['sort']):Promise<NodePage>;
  ancestors(scanId:string,nodeId:string,revision:number):Promise<Node[]>;
  chart(scanId:string,scopeId:string,revision:number):Promise<ChartData>;
  summary(scanId:string,nodeIds:string[],revision:number):Promise<SelectionSummary>;
  errors(scanId:string,cursor?:string|null):Promise<{errors:ScanError[];nextCursor:string|null}>;
  plan(request:OperationRequest):Promise<OperationPlan>; execute(planId:string,callback:(event:JobEvent)=>void):Promise<Job>; cancelOperation(jobId:string):Promise<void>;
  open(scanId:string,nodeId:string,action:'open'|'reveal'|'properties'):Promise<void>; copyPaths(scanId:string,nodeIds:string[]):Promise<string>;
  preview(request:ExportRequest):Promise<ExportPreview>; export(request:ExportRequest,callback:(event:JobEvent)=>void):Promise<Job|null>; cancelExport(jobId:string):Promise<void>; job(jobId:string):Promise<JobEvent>;
}
const progress = (callback:(event:JobEvent)=>void) => {const channel=new Channel<JobEvent>();channel.onmessage=event=>{try{callback(event);}finally{void invoke('ack_job',{jobId:event.jobId,seq:event.seq}).catch(()=>{});}};return channel;};
export class NativeDataSource implements DataSource {
  fixture=false;
  initialScanPath?:string;
  settings=()=>invoke<Settings>('read_settings'); saveSettings=(settings:Settings)=>invoke<Settings>('update_settings',{settings});
  recent=()=>invoke<Scan[]>('get_recent_scans');load=(scanId:string)=>invoke<Scan>('load_scan',{scanId});scan=(scanId:string)=>invoke<Scan>('get_scan',{scanId});
  choose=()=>invoke<Source|null>('choose_scan_source');destination=()=>invoke<Source|null>('choose_destination');
  start=(source:Source,callback:(event:JobEvent)=>void)=>invoke<Scan>('start_scan',{request:{sourceRef:source.sourceRef},onProgress:progress(callback)});
  startPath=(path:string,callback:(event:JobEvent)=>void)=>invoke<Scan>('start_scan',{request:{path},onProgress:progress(callback)});
  cancelScan=(scanId:string)=>invoke<void>('cancel_scan',{scanId});
  refresh=(scanId:string,scopeId:string,callback:(event:JobEvent)=>void)=>invoke<Scan>('refresh_scope',{scanId,scopeId,onProgress:progress(callback)});
  node=(scanId:string,nodeId:string,revision:number)=>invoke<Node>('get_node',{scanId,nodeId,revision});
  measure=(scanId:string,nodeId:string,revision:number)=>invoke<{node:Node;revision:number}>('measure_node',{scanId,nodeId,revision});
  children=(request:PageRequest)=>invoke<NodePage>('get_children',{request});search=(request:PageRequest)=>invoke<NodePage>('search_nodes',{request});
  nodePage=(scanId:string,nodeId:string,revision:number,sort:PageRequest['sort'])=>invoke<NodePage>('get_node_page',{scanId,nodeId,revision,sort,pageSize:200});
  ancestors=(scanId:string,nodeId:string,revision:number)=>invoke<Node[]>('get_ancestors',{scanId,nodeId,revision});
  chart=(scanId:string,scopeId:string,revision:number)=>invoke<ChartData>('get_chart_data',{scanId,scopeId,revision,depth:2,nodeBudget:400});
  summary=(scanId:string,nodeIds:string[],revision:number)=>invoke<SelectionSummary>('get_selection_summary',{scanId,nodeIds,revision});
  errors=(scanId:string,cursor?:string|null)=>invoke<{errors:ScanError[];nextCursor:string|null}>('get_scan_errors',{scanId,cursor,pageSize:100});
  plan=(request:OperationRequest)=>invoke<OperationPlan>('plan_operation',{request});execute=(planId:string,callback:(event:JobEvent)=>void)=>invoke<Job>('execute_operation',{planId,onProgress:progress(callback)});
  cancelOperation=(jobId:string)=>invoke<void>('cancel_operation',{jobId});job=(jobId:string)=>invoke<JobEvent>('get_job',{jobId});
  open=(scanId:string,nodeId:string,action:'open'|'reveal'|'properties')=>invoke<void>('open_node',{scanId,nodeId,action});
  copyPaths=(scanId:string,nodeIds:string[])=>invoke<string>('copy_paths',{scanId,nodeIds});preview=(request:ExportRequest)=>invoke<ExportPreview>('get_export_preview',{request});
  export=(request:ExportRequest,callback:(event:JobEvent)=>void)=>invoke<Job|null>('start_export',{request,onProgress:progress(callback)});cancelExport=(jobId:string)=>invoke<void>('cancel_export',{jobId});
}
export const bytes = (value:string|null|undefined) => value == null ? null : BigInt(value);
export const sumBytes = (nodes:Node[]) => nodes.reduce((total,node)=>total+BigInt(node.logicalTotalBytes),0n);
export function ratio(value:string|bigint,total:string|bigint):number {const a=BigInt(value),b=BigInt(total);if(b===0n)return 0;const numerator=Number(a),denominator=Number(b);if(Number.isFinite(numerator)&&Number.isFinite(denominator))return numerator/denominator;return Number(a*1000000000000000000n/b)/1e18;}
export function sizeParts(value:string|null|undefined,unit:Settings['sizeUnit']='decimal'):{value:string;unit:string} {
  if(value==null)return {value:'—',unit:''};const count=BigInt(value);const base=unit==='binary'?1024n:1000n;const units=unit==='binary'?['B','KiB','MiB','GiB','TiB','PiB','EiB']:['B','KB','MB','GB','TB','PB','EB'];let divisor=1n,index=0;
  while(count>=divisor*base&&index<units.length-1){divisor*=base;index++;}const precision=count/divisor>=100n?10n:100n;const rounded=(count*precision+divisor/2n)/divisor;const integer=rounded/precision;const fraction=(rounded%precision).toString().padStart(precision===10n?1:2,'0').replace(/0+$/,'');return {value:integer.toString()+(fraction?'.'+fraction:''),unit:units[index]};
}
export const formatSize=(value:string|null|undefined,unit:Settings['sizeUnit']='decimal')=>{const p=sizeParts(value,unit);return p.value+(p.unit?' '+p.unit:'');};
export const nodeSize=(node:Node,unit:Settings['sizeUnit']='decimal')=>node.kind==='other'&&node.errorCode&&node.logicalTotalBytes==='0'?'未知':formatSize(node.logicalTotalBytes,unit);
export const kindLabel=(node:Node)=>node.errorCode?'无法读取':node.parentId===null?'扫描根目录':node.kind==='directory'?'文件夹':node.kind==='file'?(node.extension==='gguf'?'GGUF 文件':node.extension==='safetensors'?'模型文件':'文件'):({symlink:'符号链接',junction:'目录联接',mount_point:'挂载点',other:'其他项目'}[node.kind]||'项目');
export function normalizeSelection(ids:string[],nodes:Map<string,Node>):string[] {
  const unique=[...new Set(ids)];const chosen=new Set(unique);return unique.filter(id=>{let p=nodes.get(id)?.parentId;const seen=new Set<string>();while(p&&!seen.has(p)){if(chosen.has(p))return false;seen.add(p);p=nodes.get(p)?.parentId;}return true;});
}
export function isDescendant(id:string,ancestor:string,nodes:Map<string,Node>):boolean {const seen=new Set<string>();let next:string|null=id;while(next&&!seen.has(next)){if(next===ancestor)return true;seen.add(next);next=nodes.get(next)?.parentId||null;}return false;}
export function coveredState(nodeId:string,selected:string[],nodes:Map<string,Node>):'checked'|'mixed'|'empty' {if(selected.some(id=>isDescendant(nodeId,id,nodes)))return 'checked';if(selected.some(id=>isDescendant(id,nodeId,nodes)))return 'mixed';return 'empty';}
export const palette=['var(--sf-ai)','var(--sf-games)','var(--sf-media)','var(--sf-projects)','var(--sf-downloads)','var(--sf-others)'];
export function categoryIndex(node:Node,nodes:Map<string,Node>):number {let category=node;while(category.parentId&&nodes.get(category.parentId)?.parentId)category=nodes.get(category.parentId)!;const known=['AI Models','Games','Media','Projects','Downloads','Others'].indexOf(category.name);if(known>=0)return known;let hash=0;for(const c of category.name)hash=(hash*31+c.charCodeAt(0))>>>0;return hash%palette.length;}

type FixtureDef = [string,number,FixtureDef[]?];
const fixtureDef:FixtureDef=['D:',742,[['AI Models',286,[['Checkpoints',168,[['image-base.safetensors',64],['portrait-v2.safetensors',48],['archive',32],['reference.safetensors',24]]],['LLM',78,[['vision-model.gguf',36],['language-model.gguf',28],['embeddings',14]]],['LoRA',24,[['Characters',15],['Styles',9]]],['VAE',16]]],['Games',218,[['SteamLibrary',166],['Epic Games',42],['Saves',10]]],['Media',132,[['Videos',88],['Photos',30],['Audio',14]]],['Projects',64,[['Workspace',39],['Build cache',18],['Design',7]]],['Downloads',28,[['Installers',17],['Archives',8],['Temporary',3]]],['Others',14,[['Backups',10],['Misc',4]]]]];
export function createFixture(){const nodes=new Map<string,Node>();const children=new Map<string,Node[]>();let serial=0;function add(def:FixtureDef,parent:Node|null):Node {const [name,gb,childDefs=[]]=def;const file=/\.(safetensors|gguf|bin)$/i.test(name);const total=(BigInt(gb)*1000000000n).toString();const node:Node={nodeId:String(serial++),parentId:parent?.nodeId||null,name,displayPath:parent?parent.displayPath+(parent.parentId===null?'':'\\')+name:'D:\\',kind:file?'file':'directory',extension:file?name.split('.').at(-1)!:'',depth:parent?parent.depth+1:0,logicalBytes:file?total:'0',logicalTotalBytes:total,allocatedBytes:file?total:null,allocatedKnownTotalBytes:total,allocatedUnknownCount:'0',fileCount:'0',directoryCount:String(childDefs.length),modifiedAt:'2026-10-11T10:00:00Z',identity:null,linkCount:null,linkType:null,linkTarget:null,cloudState:null,allocationMethod:'fixture',errorCode:null,incomplete:false,stale:false,childrenCount:String(childDefs.length)};nodes.set(node.nodeId,node);children.set(node.nodeId,childDefs.map(child=>add(child,node)));return node;}const root=add(fixtureDef,null);return {root,nodes,children};}
export class FixtureDataSource implements DataSource {
  fixture=true; data=createFixture(); savedSettings={...defaults};
  fixtureScan:Scan={scanId:'fixture',rootId:'0',source:'D:\\',startedAt:'2026-10-11T10:00:00Z',finishedAt:'2026-10-11T10:01:00Z',status:'completed',revision:1,discoveredCount:'34',logicalTotalBytes:'742000000000',errorCount:'0',incomplete:false,stale:false,diskSpace:{totalBytes:'1000000000000',freeBytes:'258000000000',availableBytes:'258000000000'},coverageNotes:['视觉测试专用演示数据']};
  settings=async()=>this.savedSettings;saveSettings=async(settings:Settings)=>this.savedSettings=settings;recent=async()=>[this.fixtureScan];load=async()=>this.fixtureScan;scan=async()=>this.fixtureScan;
  choose=async()=>null;destination=async()=>null;start=async()=>this.fixtureScan;cancelScan=async()=>{};refresh=async()=>this.fixtureScan;
  startPath=async()=>this.fixtureScan;
  node=async(_scanId:string,nodeId:string)=>this.data.nodes.get(nodeId)!;
  measure=async(_scanId:string,nodeId:string)=>({node:this.data.nodes.get(nodeId)!,revision:1});
  async children(request:PageRequest){const list=[...(this.data.children.get(request.parentId||'0')||[])];list.sort((a,b)=>request.sort?.startsWith('name')?(request.sort==='name_desc'?-1:1)*a.name.localeCompare(b.name):(request.sort==='size_asc'?1:-1)*(BigInt(a.logicalTotalBytes)<BigInt(b.logicalTotalBytes)?-1:BigInt(a.logicalTotalBytes)>BigInt(b.logicalTotalBytes)?1:0));const cursor=Number(request.cursor||0),pageSize=request.pageSize||200;return {revision:1,nodes:list.slice(cursor,cursor+pageSize),nextCursor:cursor+pageSize<list.length?String(cursor+pageSize):null,totalCount:String(list.length)};}
  async nodePage(scanId:string,nodeId:string,revision:number,sort:PageRequest['sort']){const node=this.data.nodes.get(nodeId)!;const all=await this.children({scanId,parentId:node.parentId||'0',revision,sort,pageSize:Number.MAX_SAFE_INTEGER});const cursor=all.nodes.findIndex(n=>n.nodeId===nodeId);const page=await this.children({scanId,parentId:node.parentId||'0',revision,sort,cursor:String(cursor),pageSize:200});return {...page,pageCursor:String(cursor)};}
  async search(request:PageRequest){const keyword=request.filter?.keyword?.toLocaleLowerCase()||'';const min=BigInt(request.filter?.minBytes||'0');const list=[...this.data.nodes.values()].filter(n=>n.parentId&&n.name.toLocaleLowerCase().includes(keyword)&&(!request.filter?.kind||request.filter.kind==='all'||n.kind===request.filter.kind)&&BigInt(n.logicalTotalBytes)>=min);return {revision:1,nodes:list,nextCursor:null,totalCount:String(list.length)};}
  async ancestors(_scanId:string,nodeId:string){const trail:Node[]=[];let p=this.data.nodes.get(nodeId)?.parentId;while(p){const n=this.data.nodes.get(p)!;trail.unshift(n);p=n.parentId;}return trail;}
  async chart(_scanId:string,scopeId:string){const build=(id:string,depth:number):ChartNode=>({node:this.data.nodes.get(id)!,children:depth<2?(this.data.children.get(id)||[]).map(child=>build(child.nodeId,depth+1)):[],otherCount:'0',otherBytes:'0'});return {revision:1,root:build(scopeId,0),truncated:false};}
  async summary(_scanId:string,ids:string[]){const normalizedIds=normalizeSelection(ids,this.data.nodes);const nodes=normalizedIds.map(id=>this.data.nodes.get(id)!);const root=nodes.some(n=>n.parentId===null);return {nodes,normalizedIds,coveredIds:ids.filter(id=>!normalizedIds.includes(id)),logicalTotalBytes:sumBytes(nodes).toString(),allocatedKnownTotalBytes:sumBytes(nodes).toString(),allocatedUnknownCount:'0',count:String(nodes.length),canOpen:nodes.length===1,canRename:nodes.length===1&&!root,canMove:!!nodes.length&&!root,canTrash:!!nodes.length&&!root};}
  errors=async()=>({errors:[],nextCursor:null});
  async plan(request:OperationRequest){const summary=await this.summary('fixture',request.nodeIds);return {planId:'fixture-plan',scanId:'fixture',revision:1,kind:request.kind,items:summary.nodes.map(node=>({nodeId:node.nodeId,name:node.name,displayPath:node.displayPath,destination:request.destinationPath||request.newName||null,error:null})),logicalTotalBytes:summary.logicalTotalBytes,expiresAt:'2099-01-01T00:00:00Z',warnings:['视觉测试模式不会操作真实文件']};}
  execute=async()=>{throw {code:'FIXTURE_READONLY',message:'视觉测试数据不执行系统文件操作',retryable:false};};cancelOperation=async()=>{};open=async()=>{};copyPaths=async(_scanId:string,ids:string[])=>ids.map(id=>this.data.nodes.get(id)!.displayPath).join('\r\n');
  async preview(request:ExportRequest){const normalized=normalizeSelection(request.selectedIds,this.data.nodes);const base=request.scope==='selected'?normalized.map(id=>this.data.nodes.get(id)!):this.data.children.get(request.scope==='all'?'0':request.scopeId)||[];const rows:Node[]=[];function walk(node:Node,children:Map<string,Node[]>){rows.push(node);if(request.recursive)(children.get(node.nodeId)||[]).forEach(n=>walk(n,children));}base.forEach(n=>walk(n,this.data.children));const total=request.scope==='selected'?sumBytes(base).toString():this.data.nodes.get(request.scope==='all'?'0':request.scopeId)!.logicalTotalBytes;return {recordCount:String(rows.length),logicalTotalBytes:total,suggestedName:'scan-report.'+request.format,incomplete:false,sample:rows.slice(0,20).map(node=>({name:node.name,path:node.displayPath,logical_bytes:node.logicalTotalBytes,kind:node.kind,parent_path:node.parentId?this.data.nodes.get(node.parentId)!.displayPath:'',depth:String(node.depth),own_logical_bytes:node.logicalBytes,allocated_bytes:node.allocatedBytes,modified_at:node.modifiedAt,node_id:node.nodeId,parent_id:node.parentId,measurement_status:'complete',link_type:node.linkType,error_code:node.errorCode}))};}
  export=async()=>{throw {code:'FIXTURE_READONLY',message:'视觉测试模式不生成实际报告',retryable:false};};cancelExport=async()=>{};job=async()=>{throw new Error('无演示任务');};
}
export async function createDataSource():Promise<DataSource> {let fixture=new URLSearchParams(location.search).get('fixture')==='1';let scanPath:string|undefined;if(isTauri()){try{const options=await invoke<{fixture:boolean;scanPath?:string}>('get_launch_options');fixture=fixture||options.fixture;scanPath=options.scanPath;}catch{/* Older shell: URL fixture remains explicit. */}}if(fixture)return new FixtureDataSource();const native=new NativeDataSource();native.initialScanPath=scanPath;return native;}
export function selectionRange(visibleIds:string[],anchor:string,target:string):string[]{const a=visibleIds.indexOf(anchor),b=visibleIds.indexOf(target);if(a<0||b<0)return [target];return visibleIds.slice(Math.min(a,b),Math.max(a,b)+1);}
