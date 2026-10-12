import { chromium } from '@playwright/test';
import { mkdirSync,writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
const port=process.env.QA_CDP_PORT||'9223';
const pid=process.env.QA_NATIVE_PID;
if(!pid)throw new Error('Set QA_NATIVE_PID to the real Spacefold process PID.');
const out=process.env.QA_OUT_DIRECTORY||'artifacts/native';mkdirSync(out,{recursive:true});
const browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const context=browser.contexts()[0];const page=context.pages().find(p=>!p.url().startsWith('devtools:'));
if(!page)throw new Error('No native WebView2 page.');
if(process.env.QA_TARGET_URL)await page.goto(process.env.QA_TARGET_URL);
else await page.reload();
await page.emulateMedia({colorScheme:'light'});
const errors=[];page.on('pageerror',e=>errors.push(String(e)));
const session=await context.newCDPSession(page);
await page.waitForFunction(()=>window.spacefoldPreview&&document.querySelector('[data-wb-row]'),null,{timeout:60000});
const evidence=[];
for(const width of [1024,736,640,639,320]){
  execFileSync('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File','scripts/resize-native.ps1','-ProcessId',pid,'-Width',String(width+2),'-Height','800'],{stdio:'pipe'});
  await session.send('Emulation.setDeviceMetricsOverride',{width:width+2,height:1000,deviceScaleFactor:1.5,mobile:false});
  await page.evaluate(target=>{
    const root=document.querySelector('#spacefold-mac'),style=getComputedStyle(root);
    root.style.setProperty('width',(target+parseFloat(style.borderLeftWidth)+parseFloat(style.borderRightWidth))+'px','important');
    const delta=target-root.clientWidth;if(delta)root.style.setProperty('width',(parseFloat(root.style.width)+delta)+'px','important');
  },width);
  await page.waitForTimeout(300);
  for(const view of ['tree','map','lens']){
    await page.evaluate(view=>window.spacefoldPreview.switchView(view),view);await page.waitForTimeout(250);
    const metric=await page.evaluate(view=>{
      const root=document.querySelector('#spacefold-mac'),panel=document.querySelector(`[data-sf-inspector="${view}"]`);
      return {view,clientWidth:root.clientWidth,viewport:innerWidth,dpr:devicePixelRatio,inspectorWidth:panel.getBoundingClientRect().width,inspectorDisplay:getComputedStyle(panel).display,scrollWidth:document.documentElement.scrollWidth,rows:document.querySelectorAll('[data-wb-row]').length,state:window.spacefoldPreview.getState()};
    },view);evidence.push(metric);
    await page.screenshot({path:`${out}/${width}-${view}.png`,scale:'css'});
    if(width<640){await page.locator(`[data-sf-inspector-toggle="${view}"]`).click();await page.screenshot({path:`${out}/${width}-${view}-drawer.png`,scale:'css'});await page.keyboard.press('Escape');}
  }
}
writeFileSync(`${out}/metrics.json`,JSON.stringify({engine:await browser.version(),kind:'Real Tauri native WebView2 window; OS DPI 144 / DPR 1.5, CSS-scale screenshots. Win32 window resizing plus CDP viewport. Application container content width is normalized using its measured 0.666667 CSS px borders. Native custom chrome is a user-requested change.',pid,url:page.url(),errors,scenarios:evidence},null,2));
console.log(JSON.stringify({count:evidence.length,errors,engine:await browser.version()}));
await browser.close();
