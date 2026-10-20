// Verify complete public installer bytes through strict HTTP range reads.
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
const [manifestPath, receiptPath] = process.argv.slice(2);
if (!manifestPath || !receiptPath) throw Error('Usage: verify_public.mjs manifest.json receipt.json');
const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
const receipts = [];
for (const item of manifest.installers) {
  const hash = createHash('sha256');
  let etag;
  const chunk = 8 * 1024 * 1024;
  for (let offset = 0; offset < item.bytes; offset += chunk * 4) {
    const tasks = Array.from({ length: 4 }, (_, i) => offset + chunk * i).filter(start => start < item.bytes);
    const parts = await Promise.all(tasks.map(async start => {
      const end = Math.min(start + chunk, item.bytes) - 1;
      let response;
      for (let attempt = 0; attempt < 3; attempt++) {
        response = await fetch(item.url, { headers: { Range: `bytes=${start}-${end}`, 'Accept-Encoding': 'identity' }, signal: AbortSignal.timeout(60000) });
        if (response.status === 206) break;
        await response.body?.cancel();
        if (attempt === 2) throw Error(`Expected range 206, got ${response.status}`);
      }
      if (response.headers.get('content-range') !== `bytes ${start}-${end}/${item.bytes}` || response.headers.get('content-encoding')) {
        await response.body?.cancel(); throw Error('Invalid public range or encoding');
      }
      const body = Buffer.from(await response.arrayBuffer());
      if (body.length !== end - start + 1) throw Error('Truncated public range');
      return { body, etag: response.headers.get('etag') };
    }));
    for (const part of parts) {
      etag ??= part.etag;
      if (!etag || part.etag !== etag) throw Error('Installer changed during readback');
      hash.update(part.body);
    }
    console.log(`${item.platform}: verified ${Math.min(offset + chunk * 4, item.bytes)}/${item.bytes} bytes`);
  }
  const sha256 = hash.digest('hex');
  if (sha256 !== item.sha256) throw Error(`Public SHA-256 mismatch for ${item.platform}`);
  receipts.push({ platform: item.platform, url: item.url, bytes: item.bytes, sha256, etag, verifiedAt: new Date().toISOString() });
}
await writeFile(receiptPath, JSON.stringify({ product: manifest.product, version: manifest.version, build: manifest.build, installers: receipts }, null, 2) + '\n');
