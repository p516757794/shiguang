// Finish only Tauri-generated NSIS files when transparent encryption blocks makensis.
// Run after `npm run tauri -- build --bundles nsis` has generated the release script.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
if (process.platform !== 'win32') throw Error('Windows only');
const root = path.resolve(__dirname, '../src-tauri/target');
const generated = path.join(root, 'release/nsis/x64');
const names = ['installer.nsi', 'SimpChinese.nsh', 'English.nsh', 'utils.nsh', 'FileAssociation.nsh'];
const entries = names.map(name => {
  const filename = path.join(generated, name);
  if (fs.lstatSync(filename).isSymbolicLink()) throw Error('Generated files must not be links');
  const bytes = fs.readFileSync(filename);
  new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  if (!bytes.length || bytes.includes(0)) throw Error('Unreadable NSIS input');
  return { path: filename, data: bytes.toString('base64'), sha: crypto.createHash('sha256').update(bytes).digest('hex') };
});
const script = fs.readFileSync(path.join(generated, 'installer.nsi'), 'utf8');
const output = script.match(/^!define OUTFILE "([^"\\/]+\.exe)"/m)?.[1];
if (!output) throw Error('Missing generated installer output');
const bridge = String.raw`
$ErrorActionPreference = 'Stop'
$entries = [Console]::In.ReadToEnd() | ConvertFrom-Json
foreach ($entry in $entries) {
  [IO.File]::WriteAllBytes($entry.path, [Convert]::FromBase64String($entry.data))
  $sha = [Security.Cryptography.SHA256]::Create()
  try { $hash = [BitConverter]::ToString($sha.ComputeHash([IO.File]::ReadAllBytes($entry.path))).Replace('-', '') } finally { $sha.Dispose() }
  if ($hash -ne $entry.sha) { throw 'Generated installer input verification failed' }
}`;
const input = JSON.stringify(entries).replace(/[^\x00-\x7f]/g, c => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0'));
const prepared = spawnSync('powershell.exe', ['-NoProfile', '-Command', bridge], { input, encoding: 'utf8', windowsHide: true });
if (prepared.status !== 0) throw Error('Cannot prepare generated installer inputs: ' + (prepared.stderr || '').slice(0, 300));
const env = { ...process.env }; delete env.NSISDIR; delete env.NSISCONFDIR;
const result = spawnSync(path.join(root, '.tauri/NSIS/makensis.exe'), ['-INPUTCHARSET', 'UTF8', '-OUTPUTCHARSET', 'UTF8', '-V3', 'installer.nsi'], { cwd: generated, env, stdio: 'inherit', windowsHide: true });
if (result.status !== 0) throw Error(`NSIS packaging failed: ${result.status}`);
const config = JSON.parse(fs.readFileSync(path.resolve(__dirname, '../src-tauri/tauri.conf.json'), 'utf8'));
const destination = path.join(root, `release/bundle/nsis/${config.productName}_${config.version}_x64-setup.exe`);
fs.mkdirSync(path.dirname(destination), { recursive: true });
// Native copy keeps the published executable readable outside Node's encryption policy.
const copyInput = JSON.stringify({ source: path.join(generated, output), destination }).replace(/[^\x00-\x7f]/g, c => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0'));
const copied = spawnSync('powershell.exe', ['-NoProfile', '-Command', "$ErrorActionPreference='Stop';$e=[Console]::In.ReadToEnd()|ConvertFrom-Json;$bytes=[IO.File]::ReadAllBytes($e.source);if($bytes[0]-ne 77-or $bytes[1]-ne 90){throw 'Unreadable installer output'};[IO.File]::WriteAllBytes($e.destination,$bytes);$actual=[IO.File]::ReadAllBytes($e.destination);if($actual.Length-ne $bytes.Length-or [Convert]::ToBase64String($actual)-ne [Convert]::ToBase64String($bytes)){throw 'Installer copy verification failed'}"], { input: copyInput, encoding: 'utf8', windowsHide: true });
if (copied.status !== 0) throw Error('Cannot copy installer: ' + (copied.stderr || '').slice(0, 300));
console.log(`Installer: ${destination}`);
