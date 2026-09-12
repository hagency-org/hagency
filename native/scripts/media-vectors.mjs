// Public deterministic test keys; no live crypto store or homeserver is opened.
import { createCipheriv, createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { Attachment, EncryptedAttachment } from '@matrix-org/matrix-sdk-crypto-nodejs';

const vectors = [];
for (const length of [0, 1, 15, 16, 17, 257, 4089, 65537]) {
  const plaintext = Buffer.from(Array.from({ length }, (_, i) => (i * 31 + 17) % 256));
  const key = Buffer.from(Array.from({ length: 32 }, (_, i) => i + 1));
  const iv = Buffer.concat([Buffer.from('0102030405060708', 'hex'), Buffer.alloc(8)]);
  const cipher = createCipheriv('aes-256-ctr', key, iv);
  const ciphertext = Buffer.concat([cipher.update(plaintext), cipher.final()]);
  const descriptor = {
    v: 'v2', key: { kty: 'oct', alg: 'A256CTR', ext: true, k: key.toString('base64url'), key_ops: ['encrypt', 'decrypt'] },
    iv: iv.toString('base64').replaceAll('=', ''),
    hashes: { sha256: createHash('sha256').update(ciphertext).digest('base64').replaceAll('=', '') },
  };
  // Exercise the same existing binding as receiveMatrixFile, not a fake decryptor.
  const checked = Buffer.from(Attachment.decrypt(new EncryptedAttachment(ciphertext, JSON.stringify(descriptor))));
  if (!checked.equals(plaintext)) throw new Error('Existing Matrix attachment crypto disagrees');
  vectors.push({ length, ciphertext: ciphertext.toString('base64'), descriptor });
}
const source = readFileSync(new URL('../../lib/matrix-file.js', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const output = JSON.stringify({ source_sha256: createHash('sha256').update(source).digest('hex'), vectors }, null, 2) + '\n';
const file = new URL('../fixtures/media.json', import.meta.url);
if (process.argv.includes('--check')) {
  if (readFileSync(file, 'utf8').replaceAll('\r\n', '\n') !== output) throw new Error('Media vectors differ');
} else writeFileSync(file, output);
console.log(JSON.stringify({ vectors: vectors.length }));
