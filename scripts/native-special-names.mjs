import {chromium} from '@playwright/test';
import {mkdirSync,writeFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const source=path.resolve('artifacts/shell-special-name'),dest=path.join(source,`copy-${randomUUID()}`);mkdirSync(dest);
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${process.env.QA_CDP_PORT||9224}`),page=browser.contexts()[0].pages().find(p=>!p.url().startsWith('devtools:'));
const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
async function wait(id,command){for(let i=0;i<1200;i++){const value=await invoke(command,command==='get_scan'?{scanId:id}:{jobId:id});if(!['running','queued','cancelling','refreshing'].includes(value.status))return value;await page.waitForTimeout(10);}throw Error('timeout');}
let scan=await invoke('start_scan',{request:{path:source}});scan=await wait(scan.scanId,'get_scan');
const nodes=(await invoke('get_children',{request:{scanId:scan.scanId,parentId:'1',revision:scan.revision}})).nodes;
const checks=[];
for(const name of ['collision.','CON.txt']){
  scan=await invoke('get_scan',{scanId:scan.scanId});const node=nodes.find(n=>n.name===name);assert(node);
  const plan=await invoke('plan_operation',{request:{scanId:scan.scanId,nodeIds:[node.nodeId],expectedRevision:scan.revision,kind:'copy',destinationPath:dest,conflictPolicy:'skip'}});
  let result;if(!plan.items[0].error){const job=await invoke('execute_operation',{planId:plan.planId});result=await wait(job.jobId,'get_job');}
  checks.push({name,plan,result});
}
const python=path.join(process.env.USERPROFILE,'.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe');
const files=JSON.parse(execFileSync(python,['-c',`import os,sys,json;root=chr(92)*2+'?'+chr(92)+sys.argv[1];print(json.dumps([{'name':n,'text':open(os.path.join(root,n),'rb').read().decode()} for n in os.listdir(root)]))`,dest],{encoding:'utf8'}));
writeFileSync(path.join(source,'results.json'),JSON.stringify({source,dest,checks,files},null,2));console.log(JSON.stringify({checks:checks.map(c=>({name:c.name,error:c.plan.items[0].error||c.result.results[0].error,status:c.result?.results[0].status})),files},null,2));
assert.equal(checks.find(c=>c.name==='collision.').plan.items[0].error?.code,'UNSUPPORTED_SHELL_PATH');
assert(!files.some(f=>f.text==='normal fixture'),'Shell resolved trailing-dot source to different normal file');await browser.close();
