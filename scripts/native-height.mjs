import {chromium,expect} from '@playwright/test';
import {execFileSync} from 'node:child_process';
import {mkdirSync,writeFileSync} from 'node:fs';
import assert from 'node:assert/strict';
const pid=process.env.QA_NATIVE_PID||'50304',port=process.env.QA_CDP_PORT||'9223';
const out=process.env.QA_OUT_DIRECTORY||'artifacts/native-height';mkdirSync(out,{recursive:true});
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`),context=browser.contexts()[0],page=context.pages().find(p=>!p.url().startsWith('devtools:'));
const session=await context.newCDPSession(page);await session.send('Emulation.clearDeviceMetricsOverride');
const errors=[];page.on('pageerror',error=>errors.push(String(error)));
await page.reload();await page.waitForFunction(()=>window.spacefoldPreview?.getScan()?.status==='completed'&&document.querySelectorAll('[data-wb-row]').length>10,null,{timeout:60000});
assert.equal(new URL(page.url()).searchParams.get('fixture'),null);
const scan=await page.evaluate(()=>window.spacefoldPreview.getScan());assert.equal(scan.discoveredCount,'501');assert.equal(scan.logicalTotalBytes,'128000');
const metrics=[];
for(const height of [600,800,1000]){
  execFileSync('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File','scripts/resize-native.ps1','-ProcessId',pid,'-Width','1026','-Height',String(height)]);
  await expect.poll(()=>page.evaluate(()=>innerHeight)).toBe(height);
  await page.locator('.wb-table-zone').evaluate(element=>element.scrollTop=0);
  await page.waitForTimeout(150);
  const measurement=await page.evaluate(()=>{
    const root=document.querySelector('#spacefold-mac'),zone=document.querySelector('.wb-table-zone'),status=document.querySelector('.wb-statusbar'),inspector=document.querySelector('[data-sf-inspector="tree"]');
    const rect=element=>{const r=element.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height,bottom:r.bottom};};
    const table=zone.getBoundingClientRect(),header=zone.querySelector('thead').getBoundingClientRect();
    const visible=[...document.querySelectorAll('[data-wb-row]')].filter(row=>{const r=row.getBoundingClientRect();return r.top>=header.bottom-.1&&r.bottom<=table.bottom+.1;});
    const row=document.querySelector('[data-wb-row]'),cells=[...row.cells].filter(cell=>getComputedStyle(cell).display!=='none');
    return {innerWidth,innerHeight,root:rect(root),zone:rect(zone),status:rect(status),inspector:rect(inspector),visibleRows:visible.length,mountedRows:document.querySelectorAll('[data-wb-row]').length,tableRight:zone.querySelector('table').getBoundingClientRect().right,rowRight:row.getBoundingClientRect().right,lastVisibleCellRight:cells.at(-1).getBoundingClientRect().right,scrollContentRight:table.left+zone.clientWidth,rootClass:root.className,documentOverflow:document.documentElement.scrollWidth-innerWidth,statusText:status.textContent,error:document.querySelector('.sf-error-notice')?.textContent};
  });
  assert(Math.abs(measurement.root.height-height)<1,'Root must fill actual native client height');assert(height-measurement.status.bottom<2,'Tree status must meet the bottom edge');assert.equal(measurement.documentOverflow,0);assert(!measurement.error);assert(measurement.mountedRows<60,'Tree must virtualize, not mount all 500');
  assert(Math.abs(measurement.rowRight-measurement.scrollContentRight)<1,'Rows must fill scroll content width');assert(Math.abs(measurement.lastVisibleCellRight-measurement.rowRight)<1,'Hidden columns must not create phantom whitespace');
  await page.screenshot({path:`${out}/${height}-tree-webview.png`,scale:'css'});
  const windowState=JSON.parse(execFileSync('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File','scripts/native-window-state.ps1','-ProcessId',pid],{encoding:'utf8'}));
  execFileSync('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File','scripts/capture-window.ps1','-WindowHandle',String(windowState.handle),'-OutputPath',`${out}/${height}-tree-window.png`]);
  const graphs=[];
  for(const view of ['map','lens']){
    await page.locator(`[data-sf-view="${view}"]`).click();await page.waitForTimeout(160);
    const graph=await page.locator(view==='map'?'.dm-map':'.dl-chart-stage').evaluate(element=>{const r=element.getBoundingClientRect();return {width:r.width,height:r.height,bottom:r.bottom};});
    assert(graph.height>100);assert(graph.bottom<=height,'Graph must remain inside native client area');graphs.push({view,...graph});await page.screenshot({path:`${out}/${height}-${view}-webview.png`,scale:'css'});
  }
  metrics.push({height,...measurement,graphs});await page.locator('[data-sf-view="tree"]').click();
}
assert(metrics[0].visibleRows<metrics[1].visibleRows&&metrics[1].visibleRows<metrics[2].visibleRows,'Real window height must change the number of visible rows');
for(const view of ['map','lens'])assert(metrics[0].graphs.find(g=>g.view===view).height<metrics[2].graphs.find(g=>g.view===view).height,'Graph must grow with real window height');
execFileSync('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File','scripts/resize-native.ps1','-ProcessId',pid,'-Width','1026','-Height','800']);
await page.waitForTimeout(180);await page.locator('[data-sf-view="tree"]').click();
writeFileSync(`${out}/metrics.json`,JSON.stringify({pid,engine:await browser.version(),mode:'Real native client resize without CDP viewport emulation; ordinary native scan, no fixture; 500 files',scan,metrics,errors},null,2));
console.log(JSON.stringify({heights:metrics.map(m=>({height:m.height,visibleRows:m.visibleRows,mountedRows:m.mountedRows,footerBottom:m.status.bottom,graphs:m.graphs})),errors}));
await browser.close();if(errors.length)process.exitCode=1;
