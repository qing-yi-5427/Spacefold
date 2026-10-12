import {chromium} from '@playwright/test';
import {mkdirSync,writeFileSync,readdirSync,existsSync} from 'node:fs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const out=path.resolve('artifacts/operation-cancellation');mkdirSync(out,{recursive:true});const source=path.join(out,randomUUID());mkdirSync(source);
for(let i=0;i<200;i++)writeFileSync(path.join(source,`trash-${String(i).padStart(3,'0')}.txt`),'owned recycle fixture');
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${process.env.QA_CDP_PORT||9224}`),page=browser.contexts()[0].pages().find(p=>!p.url().startsWith('devtools:'));
const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
async function wait(id,command){for(let i=0;i<2000;i++){const state=await invoke(command,command==='get_scan'?{scanId:id}:{jobId:id});if(!['running','queued','cancelling','refreshing'].includes(state.status))return state;await page.waitForTimeout(10);}throw Error('timeout');}
let scan=await invoke('start_scan',{request:{path:source}});scan=await wait(scan.scanId,'get_scan');
const nodes=(await invoke('get_children',{request:{scanId:scan.scanId,parentId:'1',revision:scan.revision,pageSize:200}})).nodes;assert.equal(nodes.length,200);
const plan=await invoke('plan_operation',{request:{scanId:scan.scanId,nodeIds:nodes.map(n=>n.nodeId),expectedRevision:scan.revision,kind:'trash'}});assert(plan.items.every(n=>!n.error));
const job=await invoke('execute_operation',{planId:plan.planId});const begin=performance.now();await invoke('cancel_operation',{jobId:job.jobId});const result=await wait(job.jobId,'get_job');const cancelMs=performance.now()-begin;
assert(result.results.some(n=>n.status==='cancelled'),JSON.stringify(result));assert(readdirSync(source).length>0);assert(result.results.every(n=>['success','cancelled'].includes(n.status)),JSON.stringify(result));
scan=await invoke('get_scan',{scanId:scan.scanId});const remaining=readdirSync(source).length;assert.equal(scan.discoveredCount,String(remaining+1));assert.equal(scan.logicalTotalBytes,String(remaining*21));
const evidence={source,cancelMs,result,remaining,scan,scopePreserved:true};writeFileSync(path.join(out,'results.json'),JSON.stringify(evidence,null,2));console.log(JSON.stringify({passed:true,cancelMs,remaining,status:result.status},null,2));await browser.close();
