import {chromium} from '@playwright/test';
import {mkdirSync,writeFileSync,readFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import path from 'node:path';
import assert from 'node:assert/strict';
const out=path.resolve('artifacts/native-shell');mkdirSync(out,{recursive:true});
const folder=path.join(out,randomUUID());mkdirSync(folder);
const file=path.join(folder,`spacefold-shell-${randomUUID()}.txt`);writeFileSync(file,'Spacefold association test fixture.');
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${process.env.QA_CDP_PORT||9224}`),page=browser.contexts()[0].pages().find(p=>!p.url().startsWith('devtools:'));
const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
let scan=await invoke('start_scan',{request:{path:folder}});
for(let i=0;i<1200;i++){scan=await invoke('get_scan',{scanId:scan.scanId});if(scan.status==='completed')break;await page.waitForTimeout(10);}
const children=await invoke('get_children',{request:{scanId:scan.scanId,parentId:'1',revision:scan.revision}}),node=children.nodes[0];
const checks=[];
for(const action of ['reveal','properties','open']){
  await invoke('open_node',{scanId:scan.scanId,nodeId:node.nodeId,action});
  let evidence;
  for(let i=0;i<10;i++){
    await page.waitForTimeout(250);
    evidence=JSON.parse(execFileSync('powershell.exe',['-NoProfile','-File','scripts/shell-evidence.ps1','-FixtureFile',file],{encoding:'utf8'}));
    if(evidence[action==='reveal'?'selectedByExplorer':action==='properties'?'propertiesWindow':'associatedWindow'])break;
  }
  checks.push({action,evidence,passed:evidence[action==='reveal'?'selectedByExplorer':action==='properties'?'propertiesWindow':'associatedWindow']});
  writeFileSync(path.join(out,'results.json'),JSON.stringify({file,checks},null,2));
  if(action==='properties'&&evidence.propertiesWindow)execFileSync('powershell.exe',['-NoProfile','-File','scripts/shell-evidence.ps1','-FixtureFile',file,'-CloseProperties']);
}
assert.equal(readFileSync(file,'utf8'),'Spacefold association test fixture.');
console.log(JSON.stringify({file,checks},null,2));await browser.close();assert(checks.every(check=>check.passed));
