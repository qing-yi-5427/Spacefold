import {chromium,expect} from '@playwright/test';
import {mkdirSync,writeFileSync} from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const out=path.resolve('artifacts/native-scan-performance');mkdirSync(out,{recursive:true});
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${process.env.QA_CDP_PORT||9224}`),page=browser.contexts()[0].pages().find(p=>!p.url().startsWith('devtools:'));
await page.reload();
await page.waitForFunction(()=>window.spacefoldPreview?.getScan());
assert(!new URL(page.url()).searchParams.has('fixture'));
await page.locator('[data-sf-view="tree"]').click();
await page.evaluate(()=>{window.__qaFrames=[];window.__qaLongTasks=[];window.__qaRunning=true;let previous=performance.now();function frame(now){window.__qaFrames.push(now-previous);previous=now;if(window.__qaRunning)requestAnimationFrame(frame);}requestAnimationFrame(frame);try{new PerformanceObserver(list=>window.__qaLongTasks.push(...list.getEntries().map(e=>({startTime:e.startTime,duration:e.duration})))).observe({type:'longtask',buffered:false});}catch{}});
await page.getByRole('button',{name:'选择扫描位置',exact:true}).click();await page.locator('[data-scan-path]').fill(path.resolve('artifacts/benchmark-source'));
const begin=performance.now();await page.getByRole('button',{name:'开始扫描',exact:true}).click();
const observations=[];let firstVisibleMs=null,completed;
for(let i=0;i<3000;i++){
  const snapshot=await page.evaluate(()=>{const scan=window.spacefoldPreview.getScan();return {scan,rows:document.querySelectorAll('[data-wb-row]').length,statusText:document.querySelector('[data-wb-status]')?.textContent};});
  if(snapshot.scan?.source?.endsWith('benchmark-source')){
    observations.push({elapsedMs:performance.now()-begin,...snapshot});
    if(firstVisibleMs===null&&BigInt(snapshot.scan.discoveredCount)>1n&&snapshot.rows>1)firstVisibleMs=performance.now()-begin;
    if(snapshot.scan.status==='completed'){completed=snapshot;break;}
  }
  await page.waitForTimeout(5);
}
assert(completed);assert.equal(completed.scan.discoveredCount,'100101');assert.equal(completed.scan.logicalTotalBytes,'12800000');
assert(observations.some(o=>o.scan.status==='running'&&BigInt(o.scan.discoveredCount)>1n&&o.rows>1),'No queryable partial native UI result');
const render=await page.evaluate(()=>{window.__qaRunning=false;return {maxFrameGapMs:Math.max(...window.__qaFrames),longTasks:window.__qaLongTasks,mountedRows:document.querySelectorAll('[data-wb-row]').length,windowFixture:new URL(location.href).searchParams.has('fixture')};});
assert(render.mountedRows<100);const result={firstVisibleMs,totalVisibleElapsedMs:performance.now()-begin,render,observations,passed:true};writeFileSync(path.join(out,'results.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({firstVisibleMs,totalVisibleElapsedMs:result.totalVisibleElapsedMs,render},null,2));await browser.close();
