// Capture the final native WebView2 app with real, QA-owned demonstration files.
import {chromium,expect} from '@playwright/test';
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import path from 'node:path';
import assert from 'node:assert/strict';

const out=path.resolve(process.env.QA_OUTPUT_DIRECTORY||'artifacts/perf-round2/public-demo/screenshots');
const manifest=JSON.parse(readFileSync('artifacts/perf-round2/public-demo/manifest.json','utf8').replace(/^\uFEFF/,''));
const pid=Number(process.env.QA_NATIVE_PID);
assert(Number.isSafeInteger(pid)&&pid>0,'The owned native process ID is required');
const expectedExecutable=path.resolve(process.env.QA_EXPECTED_EXE||'dist-release/Spacefold.exe').replaceAll("'","''");
execFileSync('powershell.exe',['-NoProfile','-Command',`$taskP=Get-Process -Id ${pid}; if($taskP.Path -ne '${expectedExecutable}'){throw 'Unexpected native process'}`]);
mkdirSync(out,{recursive:true});
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${process.env.QA_CDP_PORT||9232}`);
const context=browser.contexts()[0],page=context.pages().find(p=>!p.url().startsWith('devtools:'));
assert.equal(new URL(page.url()).hostname,'tauri.localhost');
assert(!new URL(page.url()).searchParams.has('fixture'));
execFileSync('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File','scripts/resize-native.ps1','-ProcessId',String(pid),'-Width','1026','-Height','800']);
await (await context.newCDPSession(page)).send('Emulation.clearDeviceMetricsOverride');
const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
const original=await invoke('read_settings');
await invoke('update_settings',{settings:{...original,theme:'light',accent:'purple',sizeUnit:'decimal',defaultView:'tree'}});
await page.reload();
await page.waitForFunction(()=>window.spacefoldPreview);
await page.getByRole('button',{name:'选择扫描位置',exact:true}).click();
await page.locator('[data-scan-path]').fill(manifest.source);
const previousScanId=await page.evaluate(()=>window.spacefoldPreview.getScan()?.scanId);
await page.getByRole('button',{name:'开始扫描',exact:true}).click();
await expect.poll(()=>page.evaluate(({source,previousScanId})=>{
  const scan=window.spacefoldPreview.getScan();
  return scan?.source===source&&scan.scanId!==previousScanId&&scan.status==='completed';
},{source:manifest.source,previousScanId})).toBe(true);
const scan=await page.evaluate(()=>window.spacefoldPreview.getScan());
assert.equal(scan.source,manifest.source);
assert.equal(scan.discoveredCount,String(manifest.discoveredCount));
assert.equal(scan.logicalTotalBytes,String(manifest.logicalBytes));
assert.equal(scan.errorCount,'0');
const screenshots=[];
for(const [view,name] of [['tree','directory-tree'],['map','space-map'],['lens','hierarchy-lens']]){
  await page.locator(`[data-sf-view="${view}"]`).click();
  await expect(page.locator(`[data-sf-view="${view}"]`)).toHaveAttribute('aria-selected','true');
  await page.waitForTimeout(300);
  const filename=path.join(out,`${name}.png`);
  await page.screenshot({path:filename,scale:'css'});
  screenshots.push({view,path:filename,state:await page.evaluate(()=>window.spacefoldPreview.getState())});
}
writeFileSync(path.join(out,'provenance.json'),JSON.stringify({actualNativeWebView:true,noFixtureMode:true,engine:await browser.version(),pid,sourceManifest:manifest,scan,screenshots},null,2));
await invoke('update_settings',{settings:original});
console.log(JSON.stringify({passed:true,scanId:scan.scanId,engine:await browser.version(),screenshots:screenshots.map(x=>x.path)},null,2));
await browser.close();
