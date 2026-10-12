import {chromium} from '@playwright/test';
import {mkdirSync,writeFileSync,readFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import path from 'node:path';
import assert from 'node:assert/strict';
const out=path.resolve(process.env.QA_OUTPUT_DIRECTORY||'artifacts/native-restart');mkdirSync(out,{recursive:true});const phase=process.argv[2]||'prepare';
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${process.env.QA_CDP_PORT||9224}`),page=browser.contexts()[0].pages().find(p=>!p.url().startsWith('devtools:'));
await page.waitForFunction(()=>window.__TAURI_INTERNALS__);
const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
async function until(id,condition){for(let i=0;i<2000;i++){const scan=await invoke('get_scan',{scanId:id});if(condition(scan))return scan;await page.waitForTimeout(5);}throw Error('scan timeout');}
if(phase==='prepare'){
  const original=await invoke('read_settings'),settings={...original,accent:'teal',theme:'dark',sizeUnit:'binary',defaultView:'lens',lastExportFormat:'xlsx'};await invoke('update_settings',{settings});
  let completed=await invoke('start_scan',{request:{path:path.resolve('artifacts/native-ui-source')}});completed=await until(completed.scanId,s=>s.status==='completed');
  let cancelled=await invoke('start_scan',{request:{path:path.resolve('artifacts/benchmark-source')}});await invoke('cancel_scan',{scanId:cancelled.scanId});cancelled=await until(cancelled.scanId,s=>s.status==='cancelled');
  let interrupted=await invoke('start_scan',{request:{path:path.resolve('artifacts/benchmark-source')}});interrupted=await until(interrupted.scanId,s=>s.status==='running'&&BigInt(s.discoveredCount)>2000n);
  writeFileSync(path.join(out,'expected.json'),JSON.stringify({original,settings,completed,cancelled,interrupted},null,2));
  await browser.close();
  execFileSync('powershell.exe',['-NoProfile','-Command',`$taskP=Get-Process -Id ${Number(process.env.QA_NATIVE_PID)}; if($taskP.Path -ne '${path.resolve(process.env.QA_EXPECTED_EXE||'dist-release/Spacefold.exe').replaceAll("'","''")}'){throw 'Unexpected process'}; Stop-Process -Id $taskP.Id`]);
  console.log('Prepared actual app process interruption and persisted settings');
}else{
  const expected=JSON.parse(readFileSync(path.join(out,'expected.json'),'utf8')),settings=await invoke('read_settings');
  for(const key of ['schemaVersion','accent','theme','sizeUnit','defaultView','lastExportFormat'])assert.equal(settings[key],expected.settings[key]);
  assert(settings.recentLocations.includes(expected.completed.source));assert(settings.recentLocations.includes(expected.interrupted.source));assert(settings.recentLocations.length<=20);
  const scans=[];for(const [key,status] of [['completed','completed'],['cancelled','cancelled'],['interrupted','interrupted']]){const scan=await invoke('load_scan',{scanId:expected[key].scanId});assert.equal(scan.status,status);assert.equal(scan.stale,true);const reread=await invoke('get_scan',{scanId:scan.scanId});assert.equal(reread.stale,true);if(key!=='completed')assert(scan.incomplete);scans.push({key,scan,reread});}
  const recent=await invoke('get_recent_scans');assert(scans.every(item=>recent.some(s=>s.scanId===item.scan.scanId)));
  const completed=scans.find(item=>item.key==='completed').reread,targetPath=path.join(out,'history-report.json');
  const exportJob=await invoke('start_export',{request:{scanId:completed.scanId,revision:completed.revision,scope:'all',scopeId:'1',selectedIds:[],recursive:true,fields:['name','logical_bytes'],format:'json',targetPath}});
  let finalExport;for(let i=0;i<1200;i++){finalExport=await invoke('get_job',{jobId:exportJob.jobId});if(!['running','queued','cancelling'].includes(finalExport.status))break;await page.waitForTimeout(5);}assert.equal(finalExport.status,'completed');
  const report=JSON.parse(readFileSync(targetPath,'utf8'));assert.equal(report.metadata.stale,true);assert.equal(report.metadata.scanStatus,'completed');
  await invoke('update_settings',{settings:expected.original});const result={actualProcessRestart:true,persistedSettings:settings,scans,historyExportMetadata:report.metadata,passed:true};writeFileSync(path.join(out,'results.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({passed:true,states:scans.map(s=>s.scan.status),historicalExportStale:report.metadata.stale},null,2));await browser.close();
}
