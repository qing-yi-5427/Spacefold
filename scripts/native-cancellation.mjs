import {chromium} from '@playwright/test';
import {mkdirSync,writeFileSync} from 'node:fs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const out=path.resolve(process.env.QA_OUTPUT_DIRECTORY||'artifacts/cancellation');mkdirSync(out,{recursive:true});
const fixture=path.join(out,randomUUID());mkdirSync(fixture);
const fileCount=Number(process.env.QA_CANCEL_FILE_COUNT||12000);assert(Number.isSafeInteger(fileCount)&&fileCount>=12000&&fileCount<=1000000);
for(let i=0;i<fileCount;i++)writeFileSync(path.join(fixture,`entry-${String(i).padStart(6,'0')}.txt`),'123');
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${process.env.QA_CDP_PORT||9223}`),page=browser.contexts()[0].pages().find(p=>!p.url().startsWith('devtools:'));
const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
const scan=await invoke('start_scan',{request:{path:fixture}});
let current;for(let i=0;i<1200;i++){current=await invoke('get_scan',{scanId:scan.scanId});if(BigInt(current.discoveredCount)>=500n||current.status==='completed')break;await page.waitForTimeout(10);}
assert(['queued','running'].includes(current.status),'Fixture finished before cancellation could be exercised');
const responseMs=await page.evaluate(()=>new Promise(resolve=>{const begin=performance.now();requestAnimationFrame(()=>resolve(performance.now()-begin));}));
const started=performance.now();await invoke('cancel_scan',{scanId:scan.scanId});
for(let i=0;i<1200;i++){current=await invoke('get_scan',{scanId:scan.scanId});if(!['queued','running','cancelling'].includes(current.status))break;await page.waitForTimeout(10);}
const cancelMs=performance.now()-started;assert.equal(current.status,'cancelled');assert(current.incomplete);assert(BigInt(current.discoveredCount)>0n);assert(cancelMs<=1000,`Local cancellation took ${cancelMs} ms`);
assert(responseMs<=100,`Renderer frame response ${responseMs} ms`);
const root=await invoke('get_node',{scanId:current.scanId,nodeId:current.rootId,revision:current.revision});
const partial=await invoke('get_children',{request:{scanId:current.scanId,parentId:current.rootId,revision:current.revision,pageSize:500}});
const retainedFiles=BigInt(current.discoveredCount)-1n;
assert.equal(root.fileCount,retainedFiles.toString());assert.equal(root.childrenCount,retainedFiles.toString());
assert.equal(root.logicalTotalBytes,(retainedFiles*3n).toString());assert.equal(partial.totalCount,retainedFiles.toString());
assert(root.incomplete);assert(partial.nodes.every(node=>node.kind==='file'&&node.logicalBytes==='3'));
const result={fixture,scan:current,cancelMs,responseMs,sourceFiles:fileCount,root,partialPageCount:partial.nodes.length,partialResultCount:partial.totalCount,partialAggregateExact:true};writeFileSync(`${out}/results.json`,JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));await browser.close();
