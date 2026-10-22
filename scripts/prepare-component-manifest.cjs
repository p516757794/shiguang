// Maintainer-only: freeze official release metadata. Never invoked by the app.
const fs = require('node:fs');
const path = require('node:path');
async function main() {
  const packages = [
    ['pip', '26.2.2', 'py3-none-any'], ['pdf2docx', '0.5.13', 'py3-none-any'],
    ['PyMuPDF', '1.28.2', 'cp310-abi3-win_amd64'], ['numpy', '2.5.4', 'cp313-cp313-win_amd64'],
    ['opencv-python-headless', '4.13.0.92', 'cp37-abi3-win_amd64'], ['python-docx', '1.2.0', 'py3-none-any'],
    ['fonttools', '4.67.0', 'py3-none-any'], ['fire', '0.7.1', 'py3-none-any'],
    ['lxml', '6.1.3', 'cp313-cp313-win_amd64'], ['typing-extensions', '4.16.0', 'py3-none-any'],
    ['termcolor', '3.3.0', 'py3-none-any'],
  ];
  const wheels = await Promise.all(packages.map(async ([name, version, tag]) => {
    const response = await fetch(`https://pypi.org/pypi/${name}/${version}/json`);
    if (!response.ok) throw Error(`Cannot resolve ${name}`);
    const metadata = await response.json();
    if (metadata.vulnerabilities?.length) throw Error(`Review advisories for ${name}`);
    const wheel = metadata.urls.find(file => file.filename.endsWith(`${tag}.whl`) && !file.yanked);
    if (!wheel || !wheel.url.startsWith('https://files.pythonhosted.org/')) throw Error(`Missing wheel ${name}`);
    return { name, version, filename: wheel.filename, url: wheel.url, sha256: wheel.digests.sha256, size: wheel.size,
      license: metadata.info.license_expression || metadata.info.license || 'See distribution LICENSE',
      source: `https://pypi.org/project/${name}/${version}/`, requires: metadata.info.requires_dist || [] };
  }));
  const pythonUrl = 'https://www.python.org/ftp/python/3.13.16/python-3.13.16-embed-amd64.zip';
  const officeUrl = 'https://download.documentfoundation.org/libreoffice/portable/26.2.4/LibreOfficePortable_26.2.4_MultilingualStandard.paf.exe';
  async function size(url) { const response = await fetch(url, { method: 'HEAD' }); if (!response.ok) throw Error(url); return Number(response.headers.get('content-length')); }
  const extractor = { bootstrap: {
    name: '7-Zip bootstrap', version: '26.05', filename: '7zr-26.05.exe', url: 'https://github.com/ip7z/7zip/releases/download/26.05/7zr.exe',
    fallbackUrls: ['https://gh-proxy.com/https://github.com/ip7z/7zip/releases/download/26.05/7zr.exe'],
    sha256: '9bf582b894dfa665f852d7e9112b02290a1c303c2d95a21fcdc41ffcb03143b7', size: 602624,
    license: 'Public domain', source: 'https://github.com/ip7z/7zip/releases/tag/26.05' }, package: {
    name: '7-Zip', version: '26.05', filename: '7z2605-x64.exe', url: 'https://www.7-zip.org/a/7z2605-x64.exe',
    fallbackUrls: ['https://github.com/ip7z/7zip/releases/download/26.05/7z2605-x64.exe', 'https://gh-proxy.com/https://github.com/ip7z/7zip/releases/download/26.05/7z2605-x64.exe'],
    sha256: 'fd99e86f57d8f429ef5ed045bf98315af962e2309b0f1ba87a147a44c71647d0', size: 1663506,
    license: 'LGPL-2.1 and bundled notices', source: 'https://github.com/ip7z/7zip/releases/tag/26.05' } };
  const manifest = { id: 'windows-x64-20261022', extractor, python: {
    name: 'Python', version: '3.13.16', filename: 'python-3.13.16-embed-amd64.zip', url: pythonUrl,
    sha256: '97dae5274cc54867065e8d5a3226e48c35017ed332a0fdb0e27d5b5821961297', size: await size(pythonUrl),
    license: 'PSF-2.0', source: 'https://www.python.org/downloads/release/python-31316/' }, office: {
    name: 'LibreOffice Portable', version: '26.2.4', filename: 'LibreOfficePortable_26.2.4_MultilingualStandard.paf.exe', url: officeUrl,
    sha256: '4bde93374aef4409243505b20d16561a4628ac7591457dd01fc6e1ccf571ba65', size: await size(officeUrl),
    license: 'MPL-2.0 and bundled third-party licenses', source: 'https://portableapps.com/apps/office/libreoffice_portable' }, wheels };
  fs.writeFileSync(path.join(__dirname, '../components/download-manifest.json'), JSON.stringify(manifest, null, 2).replace(/\n/g, '\r\n') + '\r\n', 'utf8');
  console.log(`Frozen ${wheels.length + 4} downloads, ${Math.ceil([manifest.python, manifest.office, ...wheels, extractor.bootstrap, extractor.package].reduce((sum, file) => sum + file.size, 0) / 1048576)} MiB`);
}
main().catch(error => { console.error(error.message); process.exitCode = 1; });
