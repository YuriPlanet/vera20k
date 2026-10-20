// Publish immutable native client installers using an existing authenticated
// Wrangler R2 binding. The manifest and resume files contain no credentials.
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, open, readFile, rename, stat, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const args = new Map();
for (let i = 2; i < process.argv.length; i += 2) {
  const key = process.argv[i];
  if (!key.startsWith('--') || !process.argv[i + 1]) throw Error('Expected --name value arguments');
  args.set(key.slice(2), process.argv[i + 1]);
}
for (const key of ['manifest', 'config', 'state-dir', 'wrangler-root']) {
  if (!args.has(key)) throw Error(`Missing --${key}`);
}
const manifest = JSON.parse(await readFile(args.get('manifest'), 'utf8'));
if (manifest.product !== 'vera20k-native' || !Array.isArray(manifest.installers) || manifest.installers.length !== 2) {
  throw Error('Expected the Android and iOS native installer manifest');
}
if (new Set(manifest.installers.map(item => item.platform)).size !== 2 ||
    !manifest.installers.every(item => ['android-arm64', 'ios-arm64'].includes(item.platform))) {
  throw Error('Both native mobile platforms are required');
}
const chunkSize = 8 * 1024 * 1024;
const hashFile = async file => {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
};
for (const item of manifest.installers) {
  if (!/^vera20k\/native\/[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+\.(apk|ipa)$/.test(item.key)) {
    throw Error('Installer keys must be inside an immutable native release directory');
  }
  if (!item.archiveVerified || (await stat(item.file)).size !== item.bytes || await hashFile(item.file) !== item.sha256) {
    throw Error(`Local archive verification failed: ${item.key}`);
  }
}
await mkdir(args.get('state-dir'), { recursive: true, mode: 0o700 });
const require = createRequire(path.join(path.resolve(args.get('wrangler-root')), 'package.json'));
const { getPlatformProxy } = await import(pathToFileURL(require.resolve('wrangler')).href);
const proxy = await getPlatformProxy({ configPath: path.resolve(args.get('config')), persist: false, remoteBindings: true });
try {
  const bucket = proxy.env.INSTALLERS;
  const results = [];
  for (const item of manifest.installers) {
    const existing = await bucket.head(item.key);
    if (existing) {
      if (existing.size !== item.bytes || existing.customMetadata?.sha256 !== item.sha256) throw Error(`Refusing to replace ${item.key}`);
      results.push({ key: item.key, bytes: existing.size, etag: existing.etag, sha256: item.sha256 });
      console.log(JSON.stringify({ existing: true, key: item.key, bytes: existing.size }));
      continue;
    }
    const stateFile = path.join(args.get('state-dir'), path.basename(item.key) + '.json');
    let state;
    try { state = JSON.parse(await readFile(stateFile, 'utf8')); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
    if (state && (state.key !== item.key || state.bytes !== item.bytes || state.sha256 !== item.sha256 || state.chunkSize !== chunkSize)) {
      throw Error(`Resume state does not match ${item.key}`);
    }
    const upload = state
      ? bucket.resumeMultipartUpload(item.key, state.uploadId)
      : await bucket.createMultipartUpload(item.key, {
          httpMetadata: {
            contentType: item.platform === 'android-arm64' ? 'application/vnd.android.package-archive' : 'application/octet-stream',
            contentDisposition: `attachment; filename="${path.basename(item.key)}"`,
            cacheControl: 'public, max-age=31536000, immutable',
          },
          customMetadata: { product: manifest.product, version: manifest.version, build: manifest.build, platform: item.platform, sha256: item.sha256, signing: item.signing },
        });
    state ??= { key: item.key, bytes: item.bytes, sha256: item.sha256, chunkSize, uploadId: upload.uploadId, parts: [], startedAt: new Date().toISOString() };
    let saving = Promise.resolve();
    const save = () => {
      const snapshot = JSON.stringify(state, null, 2) + '\n';
      saving = saving.then(async () => {
        await writeFile(stateFile + '.tmp', snapshot, { mode: 0o600 });
        await rename(stateFile + '.tmp', stateFile);
      });
      return saving;
    };
    await save();
    const total = Math.ceil(item.bytes / chunkSize);
    const done = new Set(state.parts.map(part => part.partNumber));
    const pending = Array.from({ length: total }, (_, i) => i + 1).filter(part => !done.has(part));
    const file = await open(item.file, 'r');
    let cursor = 0;
    try {
      const workers = await Promise.allSettled(Array.from({ length: 4 }, async () => {
        while (cursor < pending.length) {
          const partNumber = pending[cursor++];
          const offset = (partNumber - 1) * chunkSize;
          const data = Buffer.alloc(Math.min(chunkSize, item.bytes - offset));
          if ((await file.read(data, 0, data.length, offset)).bytesRead !== data.length) throw Error('Short local archive read');
          let part;
          for (let attempt = 0; attempt < 3; attempt++) {
            try { part = await upload.uploadPart(partNumber, data); break; }
            catch (error) { if (attempt === 2) throw error; await new Promise(resolve => setTimeout(resolve, 1000 * (attempt + 1))); }
          }
          if (part.etag.replaceAll('"', '') !== createHash('md5').update(data).digest('hex')) throw Error('Uploaded part checksum mismatch');
          state.parts.push({ partNumber: part.partNumber, etag: part.etag });
          await save();
          if (state.parts.length % 8 === 0 || state.parts.length === total) console.log(`${item.platform}: ${state.parts.length}/${total} parts`);
        }
      }));
      const failures = workers.filter(worker => worker.status === 'rejected');
      if (failures.length) throw new AggregateError(failures.map(worker => worker.reason), 'Upload paused; verified parts retained for resume');
    } finally { await file.close(); }
    if (state.parts.length !== total || await bucket.head(item.key)) throw Error('Cannot complete over an existing installer');
    const parts = state.parts.sort((a, b) => a.partNumber - b.partNumber);
    const complete = await upload.complete(parts);
    const actual = await bucket.head(item.key);
    const expectedETag = createHash('md5').update(Buffer.concat(parts.map(part => Buffer.from(part.etag.replaceAll('"', ''), 'hex')))).digest('hex') + '-' + total;
    if (!actual || actual.size !== item.bytes || actual.customMetadata?.sha256 !== item.sha256 || actual.etag !== complete.etag || actual.etag.replaceAll('"', '') !== expectedETag) {
      throw Error('Completed installer checksum/metadata mismatch');
    }
    const result = { key: item.key, bytes: actual.size, etag: actual.etag, sha256: item.sha256 };
    state.completedAt = new Date().toISOString(); state.remote = result;
    await save(); results.push(result); console.log(JSON.stringify({ completed: true, ...result }));
  }
  await writeFile(path.join(args.get('state-dir'), 'upload-receipt.json'), JSON.stringify({ version: manifest.version, build: manifest.build, installers: results }, null, 2) + '\n');
} finally { await proxy.dispose(); }
