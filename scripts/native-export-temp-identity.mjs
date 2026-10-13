import {chromium} from '@playwright/test';
import {mkdirSync,writeFileSync,readFileSync,readdirSync,renameSync,existsSync} from 'node:fs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const out=path.resolve(process.env.QA_OUTPUT_DIRECTORY||'artifacts/export-temp-identity',randomUUID());mkdirSync(out,{recursive:true});
const target=path.join(out,'report.csv');writeFileSync(target,'original report sentinel');
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${process.env.QA_CDP_PORT||9224}`),page=browser.contexts()[0].pages().find(p=>!p.url().startsWith('devtools:'));
const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
let scan=(await invoke('get_recent_scans')).find(s=>s.status==='completed'&&s.source.endsWith('benchmark-source'));
if(!scan){scan=await invoke('start_scan',{request:{path:path.resolve('artifacts/benchmark-source')}});for(let i=0;i<2000;i++){scan=await invoke('get_scan',{scanId:scan.scanId});if(scan.status==='completed')break;await page.waitForTimeout(5);}assert.equal(scan.status,'completed');}
const job=await invoke('start_export',{request:{scanId:scan.scanId,revision:scan.revision,scope:'all',scopeId:'1',selectedIds:[],recursive:true,fields:['name','path','logical_bytes'],format:'csv',targetPath:target}});
let temp,finalEvent;for(let i=0;i<2000;i++){temp=readdirSync(out).find(name=>name.startsWith('.tmp'));if(temp)break;finalEvent=await invoke('get_job',{jobId:job.jobId});assert(!['completed','failed','cancelled'].includes(finalEvent.status),'export finished before owned temporary replacement');await page.waitForTimeout(1);}assert(temp);
const originalTempPath=path.join(out,temp),moved=path.join(out,'moved-own-output.tmp');renameSync(originalTempPath,moved);writeFileSync(originalTempPath,'unrelated replacement sentinel');
for(let i=0;i<2000;i++){finalEvent=await invoke('get_job',{jobId:job.jobId});if(!['running','queued','cancelling'].includes(finalEvent.status))break;await page.waitForTimeout(5);}
const result={out,job:finalEvent,targetText:readFileSync(target,'utf8').slice(0,60),replacementExists:existsSync(originalTempPath),replacementText:existsSync(originalTempPath)?readFileSync(originalTempPath,'utf8'):null,movedOwnTempExists:existsSync(moved)};
writeFileSync(path.join(out,'results.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));
assert.equal(result.targetText,'original report sentinel');assert.equal(result.replacementText,'unrelated replacement sentinel');assert.equal(finalEvent.status,'failed');assert(!result.movedOwnTempExists);await browser.close();
