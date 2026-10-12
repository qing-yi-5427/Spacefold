// Generate distribution notices from the locked Windows dependency graph.
// Run cargo metadata first; no user paths are written to the output.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {execFileSync} from 'node:child_process';

const metadataPath = process.argv[2];
if (!metadataPath) throw new Error('Usage: node scripts/generate-notices.mjs <cargo-metadata.json>');
const metadata = JSON.parse(fs.readFileSync(metadataPath, 'utf8').replace(/^\uFEFF/, ''));
const lock = JSON.parse(fs.readFileSync('package-lock.json', 'utf8'));
const upstream = {
  'alloc-stdlib': ['dropbox/rust-alloc-no-stdlib', ['LICENSE']],
  'clipboard-win': ['DoumanAsh/clipboard-win', ['LICENSE']],
  'defmt-parser': ['knurling-rs/defmt', ['LICENSE-MIT', 'LICENSE-APACHE']],
  'tauri-plugin': ['tauri-apps/tauri', ['LICENSE-MIT', 'LICENSE-APACHE-2.0']],
  'webview2-com': ['wravery/webview2-rs', ['LICENSE']],
  'webview2-com-macros': ['wravery/webview2-rs', ['LICENSE']],
  'webview2-com-sys': ['wravery/webview2-rs', ['LICENSE']],
};
const groups = new Map();
const rows = [];
function addNotice(label, text, source) {
  const normalized = text.trim().replace(/\r\n/g, '\n');
  const key = crypto.createHash('sha256').update(normalized).digest('hex');
  const group = groups.get(key) || {text: normalized, labels: [], sources: new Set()};
  group.labels.push(label);
  group.sources.add(source);
  groups.set(key, group);
}
function licenseFiles(dir) {
  return fs.readdirSync(dir).filter(name => /^(licen[sc]e|copying|notice|unlicense)/i.test(name))
    .flatMap(name => {
      const file = path.join(dir, name);
      return fs.statSync(file).isFile() ? [file] : fs.readdirSync(file).map(item => path.join(file, item)).filter(item => fs.statSync(item).isFile());
    });
}
for (const pkg of metadata.packages.filter(pkg => pkg.source).sort((a,b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))) {
  const dir = path.dirname(pkg.manifest_path);
  const label = `Rust: ${pkg.name} ${pkg.version}`;
  rows.push(`| Rust | ${pkg.name} | ${pkg.version} | ${pkg.license || 'see license file'} |`);
  const files = licenseFiles(dir);
  for (const file of files) addNotice(label, fs.readFileSync(file, 'utf8'), `crate ${pkg.name} ${pkg.version}: ${path.relative(dir, file).replaceAll('\\', '/')}`);
  if (!files.length) {
    if (pkg.name === 'selectors') {
      const mpl = metadata.packages.find(item => item.name === 'cssparser');
      addNotice(label, fs.readFileSync(path.join(path.dirname(mpl.manifest_path), 'LICENSE'), 'utf8'), 'MPL-2.0 license text; selectors source: https://crates.io/crates/selectors/0.38.0');
      continue;
    }
    if (!upstream[pkg.name]) throw new Error(`Missing license: ${label}`);
    const [repo, names] = upstream[pkg.name];
    const vcs = JSON.parse(fs.readFileSync(path.join(dir, '.cargo_vcs_info.json'), 'utf8'));
    for (const name of names) {
      const url = `https://raw.githubusercontent.com/${repo}/${vcs.git.sha1}/${name}`;
      const text = execFileSync('gh', ['api', `repos/${repo}/contents/${name}?ref=${vcs.git.sha1}`, '-H', 'Accept: application/vnd.github.raw+json'], {encoding:'utf8'});
      addNotice(label, text, url);
    }
  }
}
for (const [key, pkg] of Object.entries(lock.packages).filter(([key]) => key).sort(([a],[b]) => a.localeCompare(b))) {
  const name = key.split('node_modules/').at(-1);
  rows.push(`| npm | ${name} | ${pkg.version} | ${pkg.license || 'see package'} |`);
  if (!fs.existsSync(key)) continue; // optional packages for other platforms are not distributed
  const files = licenseFiles(key);
  if (!files.length && !pkg.dev) throw new Error(`Missing production npm license: ${name}`);
  for (const file of files) addNotice(`npm: ${name} ${pkg.version}`, fs.readFileSync(file, 'utf8'), `npm ${name} ${pkg.version}: ${path.relative(key,file).replaceAll('\\', '/')}`);
}
const introduction = `# 第三方组件与许可\n\nSpacefold 源码使用 MIT；第三方组件各自遵循下列许可。此清单按 Windows x64 的 Cargo 锁定图及 npm 锁文件生成，包含构建/测试依赖，不表示全部组件均进入成品。原始许可证与版权声明在 [THIRD-PARTY-LICENSES.txt](../THIRD-PARTY-LICENSES.txt)。\n\nMPL-2.0 组件未修改，相关源码可通过 Cargo/npm 对应锁定版本取得。随 portable 分发的 Microsoft WebView2 Runtime 另遵循包内微软许可和 notices，不属于 Spacefold 的 MIT 许可。\n\n| 来源 | 组件 | 版本 | 许可 |\n|---|---|---|---|\n`;
fs.writeFileSync('docs/third-party.md', introduction + rows.join('\n') + '\n');
const sections = [...groups.values()].map(group => `${group.labels.join('\n')}\nSources:\n${[...group.sources].join('\n')}\n\n${group.text}\n`);
fs.writeFileSync('THIRD-PARTY-LICENSES.txt', `Spacefold third-party copyright and license notices\n\nGenerated from package-lock.json and the locked Windows x64 Cargo graph.\nIncludes build/test dependencies. Optional npm packages absent on this platform are not distributed.\nMicrosoft WebView2 runtime notices are supplied separately in the portable package.\n\n${sections.join('\n' + '='.repeat(78) + '\n\n')}`);
console.log(`Generated ${rows.length} dependency rows and ${groups.size} distinct notice texts.`);
