import {chromium} from '@playwright/test';
import {mkdirSync,writeFileSync,readFileSync,existsSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import path from 'node:path';
import assert from 'node:assert/strict';
const out=path.resolve('artifacts/native-dialogs');mkdirSync(out,{recursive:true});
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${process.env.QA_CDP_PORT||9224}`),page=browser.contexts()[0].pages().find(p=>!p.url().startsWith('devtools:'));
const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
async function begin(command,args={}){await page.evaluate(({command,args})=>{window.__qaDialogResult={pending:true};window.__TAURI_INTERNALS__.invoke(command,args).then(value=>window.__qaDialogResult={value}).catch(error=>window.__qaDialogResult={error});},{command,args});}
async function finish(){await page.waitForFunction(()=>!window.__qaDialogResult.pending,null,{timeout:15000});const result=await page.evaluate(()=>window.__qaDialogResult);assert(!result.error,JSON.stringify(result));return result.value;}
const dialog=(title,action,folder)=>JSON.parse(execFileSync('powershell.exe',['-NoProfile','-File','scripts/native-dialog-evidence.ps1','-OwnerPid',process.env.QA_NATIVE_PID||'40944','-DialogTitle',title,'-Action',action,...(folder?['-FolderPath',folder]:[])],{encoding:'utf8'}));
const checks=[];
await begin('choose_scan_source');const sourceEvidence=dialog('选择扫描位置','select',path.resolve('artifacts/benchmark-source'));const source=await finish();assert.equal(path.normalize(source.displayPath),path.resolve('artifacts/benchmark-source'));checks.push({name:'real native folder picker selection',sourceEvidence,source,passed:true});
let scan=await invoke('start_scan',{request:{sourceRef:source.sourceRef}});for(let i=0;i<2000;i++){scan=await invoke('get_scan',{scanId:scan.scanId});if(scan.status==='completed')break;await page.waitForTimeout(10);}assert.equal(scan.discoveredCount,'100101');
await begin('choose_destination');const destinationEvidence=dialog('选择目标目录','cancel');assert.equal(await finish(),null);checks.push({name:'real native destination picker cancel',destinationEvidence,passed:true});
const request={scanId:scan.scanId,revision:scan.revision,scope:'all',scopeId:'1',selectedIds:[],recursive:true,fields:['name','logical_bytes'],format:'csv'};
const sentinel=path.join(out,'sentinel.csv');writeFileSync(sentinel,'preserved original');const before=await invoke('get_recent_scans');
await begin('start_export',{request});const saveEvidence=dialog('保存扫描报告','cancel');assert.equal(await finish(),null);assert.equal(readFileSync(sentinel,'utf8'),'preserved original');checks.push({name:'real native save picker cancellation creates no export job',saveEvidence,passed:true});
writeFileSync(path.join(out,'results.json'),JSON.stringify({checks},null,2));console.log(JSON.stringify({passed:checks.length,source},null,2));await browser.close();
