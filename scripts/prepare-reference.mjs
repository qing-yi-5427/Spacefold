import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
const doc = readFileSync('spacefold-ui-spec.md', 'utf8').replace(/\r\n/g, '\n');
const fragment = doc.split('\n<!-- SPACEFOLD_REFERENCE_BEGIN -->\n````html\n')[1].split('\n````\n<!-- SPACEFOLD_REFERENCE_END -->')[0];
const digest = createHash('sha256').update(fragment).digest('hex');
if (digest !== '898f933a7dd8f3fb50b4b086e53ef45122c936981fbdbcf18325dd89d98b7f2e') throw new Error(`Reference hash changed: ${digest}`);
mkdirSync('reference', {recursive:true});
writeFileSync('reference/spacefold-reference.html', fragment);
const lucide = readFileSync('node_modules/lucide/dist/umd/lucide.js', 'utf8');
writeFileSync('reference/index.html', `<!doctype html><html lang="zh-CN"><meta charset="UTF-8"><title>Spacefold 固定参考</title><style>:root{color-scheme:light}body{margin:0}#spacefold-mac{box-sizing:content-box;width:calc(100% - 2px)!important}</style><script>${lucide}</script>${fragment}<script>window.lucide.createIcons();</script></html>`);
console.log(JSON.stringify({sha256:digest, bytes:Buffer.byteLength(fragment)}));

