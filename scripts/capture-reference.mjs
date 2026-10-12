import { chromium } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
const out = process.env.QA_OUT_DIRECTORY||'artifacts/reference'; mkdirSync(out, {recursive:true});
const browser = await chromium.launch({channel:'msedge',headless:true});
const context = await browser.newContext({deviceScaleFactor:1, colorScheme:'light'});
const page = await context.newPage();
const evidence=[];
for (const width of [1024,736,640,639,320]) {
  await page.setViewportSize({width:width+2,height:800});
  await page.goto(process.env.QA_TARGET_URL||'http://127.0.0.1:1420/reference/index.html');
  await page.waitForFunction(()=>!!window.spacefoldPreview&&document.querySelectorAll('[data-wb-row]').length===14);
  for (const view of ['tree','map','lens']) {
    await page.evaluate(view=>window.spacefoldPreview.switchView(view),view);
    await page.waitForTimeout(250);
    const metrics=await page.evaluate(view=>{
      const root=document.getElementById('spacefold-mac');
      const inspector=document.querySelector(`[data-sf-inspector="${view}"]`);
      return {view,width:root.clientWidth,dpr:devicePixelRatio,inspectorWidth:inspector.getBoundingClientRect().width,inspectorDisplay:getComputedStyle(inspector).display,rootHeight:root.getBoundingClientRect().height,scrollWidth:document.documentElement.scrollWidth, viewport:innerWidth,rows:document.querySelectorAll('[data-wb-row]').length};
    },view);
    evidence.push(metrics);
    await page.screenshot({path:`${out}/${width}-${view}.png`,fullPage:true});
    if(width<640){
      await page.locator(`[data-sf-inspector-toggle="${view}"]`).click();
      await page.screenshot({path:`${out}/${width}-${view}-drawer.png`,fullPage:true});
      await page.keyboard.press('Escape');
    }
  }
}
await page.setViewportSize({width:1026,height:800});
await page.emulateMedia({colorScheme:'dark'});
await page.goto(process.env.QA_TARGET_URL||'http://127.0.0.1:1420/reference/index.html');
await page.waitForFunction(()=>!!window.spacefoldPreview&&document.querySelectorAll('[data-wb-row]').length===14);
await page.evaluate(()=>document.documentElement.style.colorScheme='dark');
for(const view of ['tree','map','lens']){
  await page.evaluate(view=>window.spacefoldPreview.switchView(view),view);
  await page.waitForTimeout(250);
  await page.screenshot({path:`${out}/1024-${view}-dark.png`,fullPage:true});
}
writeFileSync(`${out}/metrics.json`,JSON.stringify({engine:await browser.version(),kind:'Edge browser reference, not native application',scenarios:evidence},null,2));
console.log(JSON.stringify(evidence,null,2));
await browser.close();
