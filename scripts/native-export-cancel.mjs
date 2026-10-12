import {chromium} from '@playwright/test';
import {mkdirSync,writeFileSync,readFileSync,readdirSync} from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const out=path.resolve('artifacts/export-cancellation');mkdirSync(out,{recursive:true});
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${process.env.QA_CDP_PORT||9224}`),page=browser.contexts()[0].pages().find(p=>!p.url().startsWith('devtools:'));
const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
async function waitScan(id){for(let i=0;i<1200;i++){const scan=await invoke('get_scan',{scanId:id});if(!['running','queued','cancelling'].includes(scan.status))return scan;await page.waitForTimeout(10);}throw Error('scan timeout');}
let scan=await invoke('start_scan',{request:{path:path.resolve('artifacts/benchmark-source')}});scan=await waitScan(scan.scanId);assert.equal(scan.discoveredCount,'100101');
const checks=[];
for(const format of ['csv','xlsx','json','html']){
  const target=path.join(out,`preserve.${format}`);writeFileSync(target,'original target sentinel');
  const before=new Set(readdirSync(out));
  const request={scanId:scan.scanId,revision:scan.revision,scope:'all',scopeId:'1',selectedIds:[],recursive:true,fields:['name','path','logical_bytes','node_id'],format,targetPath:target,htmlRowBudget:200000};
  const job=await invoke('start_export',{request});assert(job);
  let current=await invoke('get_job',{jobId:job.jobId});
  assert(['running','queued'].includes(current.status),`${format} already ${current.status}`);
  const begin=performance.now();await invoke('cancel_export',{jobId:job.jobId});
  for(let i=0;i<2000;i++){current=await invoke('get_job',{jobId:job.jobId});if(!['running','queued','cancelling'].includes(current.status))break;await page.waitForTimeout(10);}
  const cancelMs=performance.now()-begin;
  assert.equal(current.status,'cancelled',JSON.stringify(current));assert.equal(readFileSync(target,'utf8'),'original target sentinel');
  assert.deepEqual(readdirSync(out).filter(name=>!before.has(name)),[],'Unexpected temporary output remained');
  checks.push({format,phaseAtCancel:current.phase,cancelMs,targetPreserved:true,event:current});writeFileSync(path.join(out,'results.json'),JSON.stringify({scan,checks},null,2));
}
console.log(JSON.stringify({passed:checks.length,checks:checks.map(({format,cancelMs,targetPreserved})=>({format,cancelMs,targetPreserved}))},null,2));await browser.close();
