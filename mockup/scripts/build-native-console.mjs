/* Build tooling only. Deploy the manifest directory with the Rust executable.
 * A retained font cache can replay actual downloaded fonts without network. */
import { cp, mkdir, mkdtemp, readFile, readdir, realpath, stat, symlink, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const source = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const outputAt = args.indexOf('--output');
const cacheAt = args.indexOf('--font-cache');
if (outputAt < 0 || !args[outputAt + 1]) throw new Error('Usage: build-native-console.mjs --output <new directory> [--font-cache <existing .next directory>]');
const output = resolve(args[outputAt + 1]);
await mkdir(output, { mode: 0o700 }); // Refuse existing output, retaining failed artifacts.
const work = await mkdtemp(join(dirname(output), 'native-console-build-'));
const staged = join(work, 'mockup');
await mkdir(join(staged, 'app', 'usage'), { recursive: true, mode: 0o700 });
await mkdir(join(staged, 'app', 'resources', 'new'), { recursive: true, mode: 0o700 });
for (const name of ['components', 'lib', 'package.json', 'jsconfig.json', 'next.config.mjs']) await cp(join(source, name), join(staged, name), { recursive: true });
for (const name of ['layout.jsx', 'globals.css', 'usage/page.jsx', 'resources/page.jsx', 'resources/new/page.jsx']) await cp(join(source, 'app', name), join(staged, 'app', name));
await mkdir(join(work, 'lib'), { mode: 0o700 });
await cp(join(source, '..', 'lib', 'role-capacity.json'), join(work, 'lib', 'role-capacity.json'));
await symlink(await realpath(join(source, 'node_modules')), join(staged, 'node_modules'), 'dir');
const env = Object.fromEntries(['PATH', 'HOME', 'TMPDIR', 'LANG'].filter((k) => process.env[k]).map((k) => [k, process.env[k]]));
Object.assign(env, { NEXT_TELEMETRY_DISABLED: '1', NEXT_PUBLIC_HAGENCY_NATIVE_CONSOLE: '1' });
if (cacheAt >= 0) {
  const cache = resolve(args[cacheAt + 1]);
  const families = { Roboto: [], 'Noto Sans SC': [] };
  for (const file of await readdir(join(cache, 'static', 'chunks'))) {
    if (!file.endsWith('.css')) continue;
    const cssPath = join(cache, 'static', 'chunks', file);
    const css = await readFile(cssPath, 'utf8');
    for (const match of css.matchAll(/@font-face\{[^}]+\}/g)) {
      const family = /font-family:([^;]+);/.exec(match[0])?.[1]?.replaceAll('"', '').replaceAll("'", '').trim();
      if (!families[family] || !match[0].includes('src:url(')) continue;
      const block = match[0].replace(/src:url\(([^)]+)\)/, (_, path) => `src: url(${resolve(dirname(cssPath), path)})`).replaceAll(';', ';\n');
      families[family].push(block);
    }
  }
  if (Object.values(families).some((rows) => rows.length === 0)) throw new Error('Font cache lacks the retained layout fonts');
  const replay = join(work, 'retained-fonts.cjs');
  await writeFile(replay, `const fonts=${JSON.stringify(Object.fromEntries(Object.entries(families).map(([k, v]) => [k, v.join('\n')])))}; module.exports=new Proxy({}, {get(_, url) { if(typeof url!=='string') return undefined; const family=new URL(url).searchParams.get('family')?.split(':')[0]; return fonts[family]; }});`, { mode: 0o600 });
  env.NEXT_FONT_GOOGLE_MOCKED_RESPONSES = replay;
  console.log('Replaying the retained layout’s actual local font bytes; no font network requests.');
}
console.log(`Native build staging: ${work}`);
const next = join(staged, 'node_modules', 'next', 'dist', 'bin', 'next');
const child = spawn(process.execPath, [next, 'build', '--webpack'], { cwd: staged, env, stdio: 'inherit' });
const code = await new Promise((done, reject) => { child.once('error', reject); child.once('exit', done); });
if (code !== 0) throw new Error(`Next build failed (${code}); staging retained at ${work}`);
const exported = join(staged, 'out');
const files = ['usage/index.html', 'resources/index.html', 'resources/new/index.html'];
async function walk(dir, relative) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = `${relative}/${entry.name}`;
    if (entry.isDirectory()) await walk(join(dir, entry.name), path);
    else if (entry.isFile()) files.push(path);
    else throw new Error('Build contains a link or non-file');
  }
}
await walk(join(exported, '_next', 'static'), '_next/static');
const mime = (path) => ['usage/index.html', 'resources/index.html', 'resources/new/index.html'].includes(path) ? 'text/html; charset=utf-8'
  : path.endsWith('.js') ? 'text/javascript; charset=utf-8' : path.endsWith('.css') ? 'text/css; charset=utf-8'
    : path.endsWith('.woff2') ? 'font/woff2' : path.endsWith('.woff') ? 'font/woff' : null;
let total = 0; let largest = 0; const assets = [];
for (const path of files.sort()) {
  const type = mime(path);
  if (!type) throw new Error(`Unexpected exported asset type: ${path}`);
  const bytes = await readFile(join(exported, path));
  total += bytes.length; largest = Math.max(largest, bytes.length);
  assets.push({ path, size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex'), mime: type });
}
console.log(JSON.stringify({ count: assets.length, total_bytes: total, largest_bytes: largest }));
if (assets.length > 512 || total > 32 * 1024 * 1024 || largest > 4 * 1024 * 1024) throw new Error('Native asset limits exceeded; no required chunk was excluded');
const manifest = JSON.stringify({ version: 1, assets });
if (Buffer.byteLength(manifest) > 128 * 1024) throw new Error('Native manifest limit exceeded');
for (const asset of assets) {
  await mkdir(dirname(join(output, asset.path)), { recursive: true, mode: 0o700 });
  await writeFile(join(output, asset.path), await readFile(join(exported, asset.path)), { flag: 'wx', mode: 0o600 });
  if ((await stat(join(output, asset.path))).size !== asset.size) throw new Error('Asset changed while packaging');
}
await writeFile(join(output, 'manifest.json'), manifest, { flag: 'wx', mode: 0o600 });
console.log(`Native console assets: ${output}`);
