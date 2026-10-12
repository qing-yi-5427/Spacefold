import {chromium} from '@playwright/test';
import {mkdirSync,writeFileSync,readFileSync,linkSync,symlinkSync,unlinkSync,existsSync,readdirSync,lstatSync,openSync,closeSync,ftruncateSync} from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {execFileSync,spawnSync} from 'node:child_process';
const out=path.resolve('artifacts/backend');mkdirSync(out,{recursive:true});
const fixture=path.join(out,`fixture-${randomUUID()}`),source=path.join(fixture,'source'),destination=path.join(fixture,'destination');
mkdirSync(source,{recursive:true});mkdirSync(destination);
for(const dirname of ['Data','Database','空目录'])mkdirSync(path.join(source,dirname));
writeFileSync(path.join(source,'Data','a.bin'),Buffer.alloc(3001,42));
writeFileSync(path.join(source,'Database','b.txt'),'second sibling');
writeFileSync(path.join(source,'zero.bin'),'');
writeFileSync(path.join(source,'=SUM(1+1).txt'),'formula name');
writeFileSync(path.join(source,'中文 🗂️ &组合é.txt'),'Unicode fixture');
linkSync(path.join(source,'Data','a.bin'),path.join(source,'hardlink.bin'));
let deep=source;for(let index=0;index<14;index++){deep=path.join(deep,`long-segment-${index.toString().padStart(2,'0')}-中文`);mkdirSync(deep);}writeFileSync(path.join(deep,'末尾.txt'),'long-path-data');
symlinkSync(source,path.join(source,'Data','loop'),'junction');
const sparse=path.join(source,'sparse.bin');writeFileSync(sparse,'');execFileSync('fsutil.exe',['sparse','setflag',sparse]);const sparseHandle=openSync(sparse,'r+');ftruncateSync(sparseHandle,2**31);closeSync(sparseHandle);
const compressed=path.join(source,'compressed.bin');writeFileSync(compressed,Buffer.alloc(1024*1024,65));execFileSync('compact.exe',['/c','/i',compressed]);
let expected=0n;function total(folder){for(const entry of readdirSync(folder)){const file=path.join(folder,entry),m=lstatSync(file);if(m.isSymbolicLink())continue;if(m.isDirectory())total(file);else expected+=BigInt(m.size);}}total(source);
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${process.env.QA_CDP_PORT||9223}`);
const page=browser.contexts()[0].pages().find(p=>!p.url().startsWith('devtools:'));
const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
const checks=[];async function check(name,fn){const start=performance.now();const data=await fn();checks.push({name,passed:true,elapsedMs:Math.round(performance.now()-start),data});console.log(`PASS ${name}`);}
async function waitScan(id){for(let i=0;i<1200;i++){const scan=await invoke('get_scan',{scanId:id});if(!['queued','running','cancelling'].includes(scan.status))return scan;await page.waitForTimeout(25);}throw Error('scan timeout');}
async function waitJob(id){for(let i=0;i<1200;i++){const job=await invoke('get_job',{jobId:id});if(!['queued','running','cancelling','refreshing'].includes(job.status))return job;await page.waitForTimeout(25);}throw Error('job timeout');}
let scan=await invoke('start_scan',{request:{path:source}});scan=await waitScan(scan.scanId);
let children;
await check('scan real Unicode, long paths, hard links, junction loop and zero',async()=>{assert.equal(scan.logicalTotalBytes,expected.toString());assert.equal(scan.status,'completed');children=await invoke('get_children',{request:{scanId:scan.scanId,parentId:scan.rootId,revision:scan.revision,pageSize:500}});assert.equal(children.nodes.find(n=>n.name==='zero.bin').logicalTotalBytes,'0');return scan;});
async function measure(id){const measured=await invoke('measure_node',{scanId:scan.scanId,nodeId:id,revision:scan.revision});scan=await invoke('get_scan',{scanId:scan.scanId});return measured.node;}
await check('hardlink identity and count measured on demand',async()=>{const hard=await measure(children.nodes.find(n=>n.name==='hardlink.bin').nodeId);assert.equal(hard.linkCount,'2');assert(hard.identity);return hard;});
const data=children.nodes.find(n=>n.name==='Data'),database=children.nodes.find(n=>n.name==='Database');
const dataChildren=await invoke('get_children',{request:{scanId:scan.scanId,parentId:data.nodeId,revision:scan.revision,pageSize:500}}),a=dataChildren.nodes.find(n=>n.name==='a.bin');
await check('ancestor normalization uses IDs, Data versus Database',async()=>{const summary=await invoke('get_selection_summary',{scanId:scan.scanId,nodeIds:[data.nodeId,a.nodeId,database.nodeId],revision:scan.revision});assert.equal(summary.count,'2');assert.equal(summary.logicalTotalBytes,(BigInt(data.logicalTotalBytes)+BigInt(database.logicalTotalBytes)).toString());assert(summary.coveredIds.includes(a.nodeId));return summary;});
await check('junction not followed',async()=>{const loop=dataChildren.nodes.find(n=>n.name==='loop');assert.equal(loop.kind,'junction');assert.equal(loop.logicalTotalBytes,'0');return loop;});
await check('sparse and compressed allocation are platform measurements',async()=>{const sp=await measure(children.nodes.find(n=>n.name==='sparse.bin').nodeId),co=await measure(children.nodes.find(n=>n.name==='compressed.bin').nodeId);assert.equal(sp.logicalBytes,String(2**31));assert(BigInt(sp.allocatedBytes)<BigInt(sp.logicalBytes));assert(BigInt(co.allocatedBytes)<BigInt(co.logicalBytes));assert.equal(sp.allocationMethod,'FILE_STANDARD_INFO.AllocationSize');return {sparse:sp,compressed:co};});
await check('search by path, type and minimum',async()=>{const page=await invoke('search_nodes',{request:{scanId:scan.scanId,revision:scan.revision,filter:{keyword:'Database',kind:'file',minBytes:'1'}}});assert(page.nodes.some(n=>n.name==='b.txt'));return page.totalCount;});
await check('real Windows clipboard roundtrip',async()=>{const original=execFileSync('powershell.exe',['-NoProfile','-Command','Get-Clipboard -Raw'],{encoding:'utf8'});try{const value=await invoke('copy_paths',{scanId:scan.scanId,nodeIds:[a.nodeId]});const actual=execFileSync('powershell.exe',['-NoProfile','-Command','Get-Clipboard -Raw'],{encoding:'utf8'}).trimEnd();assert.equal(actual,value.trimEnd());return {textMatches:true};}finally{spawnSync('powershell.exe',['-NoProfile','-Command','$taskText=[Console]::In.ReadToEnd(); Set-Clipboard -Value $taskText'],{input:original,encoding:'utf8'});}});
const fields=['name','path','logical_bytes','kind','parent_path','depth','own_logical_bytes','allocated_bytes','modified_at','node_id','measurement_status','link_type','error_code'];
await check('four real exports and shared preview count',async()=>{let count;for(const format of ['csv','xlsx','json','html']){const request={scanId:scan.scanId,revision:scan.revision,scope:'all',scopeId:scan.rootId,selectedIds:[],recursive:true,fields,format,targetPath:path.join(out,`report.${format}`)};const preview=await invoke('get_export_preview',{request});count=preview.recordCount;const job=await invoke('start_export',{request}),result=await waitJob(job.jobId);assert.equal(result.status,'completed');assert(existsSync(request.targetPath));}return {count};});
await check('selected export excludes duplicate child',async()=>{const preview=await invoke('get_export_preview',{request:{scanId:scan.scanId,revision:scan.revision,scope:'selected',scopeId:scan.rootId,selectedIds:[data.nodeId,a.nodeId,database.nodeId],recursive:false,fields,format:'json'}});assert.equal(preview.recordCount,'2');assert.equal(preview.logicalTotalBytes,(BigInt(data.logicalTotalBytes)+BigInt(database.logicalTotalBytes)).toString());return preview;});
await check('real copy keeps source and content',async()=>{const plan=await invoke('plan_operation',{request:{scanId:scan.scanId,nodeIds:[a.nodeId],expectedRevision:scan.revision,kind:'copy',destinationPath:destination,conflictPolicy:'skip'}});assert.equal(plan.items[0].error,null);const job=await invoke('execute_operation',{planId:plan.planId}),result=await waitJob(job.jobId);assert.equal(result.results[0].status,'success');assert.deepEqual(readFileSync(path.join(destination,'a.bin')),readFileSync(path.join(source,'Data','a.bin')));scan=await invoke('get_scan',{scanId:scan.scanId});return result;});
await check('stale source replacement is rejected',async()=>{scan=await invoke('get_scan',{scanId:scan.scanId});const current=await invoke('search_nodes',{request:{scanId:scan.scanId,revision:scan.revision,filter:{keyword:'zero.bin',kind:'file'}}});const zero=current.nodes.find(n=>n.name==='zero.bin');const plan=await invoke('plan_operation',{request:{scanId:scan.scanId,nodeIds:[zero.nodeId],expectedRevision:scan.revision,kind:'move',destinationPath:destination,conflictPolicy:'skip'}});unlinkSync(path.join(source,'zero.bin'));writeFileSync(path.join(source,'zero.bin'),'changed identity and size');const job=await invoke('execute_operation',{planId:plan.planId}),result=await waitJob(job.jobId);assert.equal(result.results[0].status,'failed');assert.equal(result.results[0].error.code,'SOURCE_CHANGED');assert(existsSync(path.join(source,'zero.bin')));return result;});
writeFileSync(`${out}/results.json`,JSON.stringify({fixture,expectedBytes:expected.toString(),checks},null,2));
console.log(JSON.stringify({passed:checks.length,fixture}));await browser.close();
