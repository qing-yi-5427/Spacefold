import {chromium} from '@playwright/test';
import {mkdirSync,writeFileSync} from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';

const out=path.resolve(process.env.QA_OUTPUT_DIRECTORY||'artifacts/native-concurrent-reads');
mkdirSync(out,{recursive:true});
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${process.env.QA_CDP_PORT||9223}`);
const page=browser.contexts()[0].pages().find(p=>!p.url().startsWith('devtools:'));
assert.equal(new URL(page.url()).hostname,'tauri.localhost');
assert(!new URL(page.url()).searchParams.has('fixture'));
const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
const source=path.resolve('artifacts/benchmark-source');
const scan=await invoke('start_scan',{request:{path:source}});
const started=performance.now();
const lanes=32,readsPerLane=50,observations=[];
await Promise.all(Array.from({length:lanes},async(_,lane)=>{
  let previousCount=0n,previousRevision=0;
  for(let ordinal=0;ordinal<readsPerLane;ordinal++){
    const current=await invoke('get_scan',{scanId:scan.scanId});
    assert(BigInt(current.discoveredCount)>=previousCount);
    assert(current.revision>=previousRevision);
    assert.equal(current.errorCount,'0');
    previousCount=BigInt(current.discoveredCount);previousRevision=current.revision;
    observations.push({lane,ordinal,elapsedMs:performance.now()-started,status:current.status,
      discoveredCount:current.discoveredCount,revision:current.revision});
  }
}));
let terminal;
for(let count=0;count<1000;count++){
  terminal=await invoke('get_scan',{scanId:scan.scanId});
  if(!['queued','running','cancelling'].includes(terminal.status))break;
  await page.waitForTimeout(10);
}
assert.equal(terminal.status,'completed');
assert.equal(terminal.discoveredCount,'100101');
assert.equal(terminal.logicalTotalBytes,'12800000');
assert.equal(terminal.errorCount,'0');
assert(!terminal.incomplete);
assert(observations.some(value=>value.status==='running'&&BigInt(value.discoveredCount)>1n),
  'The concurrent read workload did not observe committed partial scanning');
const result={passed:true,source,scan:terminal,outstandingClientLanes:lanes,readsPerLane,
  totalSuccessfulReadCalls:lanes*readsPerLane,backendCommandBudget:4,
  readerBoundary:'32 outstanding client lanes; production command adapter limits simultaneous blocking work to four',
  elapsedMs:performance.now()-started,observations};
writeFileSync(path.join(out,'results.json'),JSON.stringify(result,null,2));
console.log(JSON.stringify({...result,observations:undefined},null,2));
await browser.close();
