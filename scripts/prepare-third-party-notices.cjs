// Maintainer helper: collect local dependency notices without downloading or executing packages.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
const root = path.resolve(__dirname, '..');
const unique = new Map();
const packages = [];
function collect(name, directory, license, repository) {
  const files = fs.readdirSync(directory).filter(file => /^(licen[cs]e|copying|copyright|notice)([-_.]|$)/i.test(file) && fs.statSync(path.join(directory, file)).isFile());
  const refs = [];
  for (const file of files) {
    const text = fs.readFileSync(path.join(directory, file), 'utf8').replace(/\r\n/g, '\n').trim();
    if (text.includes('\0') || text.includes('\uFFFD')) throw Error(`Unreadable notice: ${name}/${file}`);
    const digest = crypto.createHash('sha256').update(text).digest('hex');
    if (!unique.has(digest)) unique.set(digest, { id: `notice-${unique.size + 1}`, text });
    refs.push(`[${file}](#${unique.get(digest).id})`);
  }
  packages.push(`- **${name}** — ${license || '见原文件'}${repository ? ` · ${repository}` : ''}\n  ${refs.length ? refs.join(' · ') : '本地包未提供根目录许可文件；请根据包内文档及原始来源核对。'}`);
}
const lock = JSON.parse(fs.readFileSync(path.join(root, 'package-lock.json'), 'utf8'));
for (const relative of Object.keys(lock.packages)) {
  if (!relative) continue;
  const directory = path.join(root, relative);
  if (!fs.existsSync(path.join(directory, 'package.json'))) continue;
  const pkg = JSON.parse(fs.readFileSync(path.join(directory, 'package.json'), 'utf8'));
  collect(`${pkg.name} ${pkg.version}`, directory, pkg.license, typeof pkg.repository === 'string' ? pkg.repository : pkg.repository?.url);
}
const result = spawnSync('cargo', ['metadata', '--format-version', '1', '--offline', '--locked', '--filter-platform', 'x86_64-pc-windows-msvc'], { cwd: path.join(root, 'src-tauri'), encoding: 'utf8', windowsHide: true, timeout: 30000, maxBuffer: 12 * 1024 * 1024 });
if (result.status !== 0) throw Error(`Cannot read Cargo metadata: ${(result.stderr || '').slice(0, 300)}`);
for (const pkg of JSON.parse(result.stdout).packages) {
  if (pkg.source?.startsWith('registry+')) collect(`${pkg.name} ${pkg.version}`, path.dirname(pkg.manifest_path), pkg.license, pkg.repository);
}
const intro = '# 第三方依赖与许可\n\n拾光自身采用 AGPL-3.0-only，见 LICENSE。以下收集当前锁定的运行与构建依赖的原始许可与版权说明。相同文本去重后以编号引用，原始版权声明保留。第三方依赖各自适用原协议。可选转换组件另见 components/COMPONENT-NOTICES.md。\n\n## 依赖索引\n\n';
const notices = [...unique.values()].map(item => `<a id="${item.id}"></a>\n\n## ${item.id}\n\n${item.text}`).join('\n\n');
const text = intro + packages.join('\n\n') + '\n\n' + notices + '\n';
fs.writeFileSync(path.join(root, 'THIRD_PARTY_NOTICES.md'), text.replace(/\r?\n/g, '\r\n'), 'utf8');
console.log(`${packages.length} packages; ${unique.size} unique notice files; ${Buffer.byteLength(text)} bytes`);
