// Export generated frontend assets or the fixed component manifest through Node,
// which can read them in transparent-encryption environments. No user data.
const fs = require('node:fs');
const path = require('node:path');
if (process.argv.includes('--component-manifest')) {
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, '../components/download-manifest.json'), 'utf8'));
  if (!manifest.id || !manifest.python || !manifest.office || !manifest.extractor?.bootstrap || !manifest.extractor?.package || !Array.isArray(manifest.wheels)) throw new Error('Invalid component manifest');
  process.stdout.write(JSON.stringify(manifest));
  process.exit(0);
}
const root = path.resolve(__dirname, '../dist');
const files = [];
function visit(directory) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const filename = path.join(directory, entry.name);
    if (entry.isSymbolicLink()) throw new Error('Build assets must not contain links');
    if (entry.isDirectory()) visit(filename);
    else if (entry.isFile()) {
      const bytes = fs.readFileSync(filename);
      files.push({ path: path.relative(root, filename), data: bytes.toString('base64') });
    }
  }
}
const html = fs.readFileSync(path.join(root, 'index.html'), 'utf8');
if (!/^<!doctype html>/i.test(html) || !html.includes('charset="UTF-8"')) {
  throw new Error('Generated index.html is not valid UTF-8 HTML');
}
visit(root);
process.stdout.write(JSON.stringify(files));
