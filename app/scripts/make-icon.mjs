// Writes src-tauri/app-icon.png: a placeholder 1024x1024 RGBA tile (the notification tile gradient
// from the mock). `npm run icons` then runs `tauri icon` to derive src-tauri/icons/*.
// Replace with real artwork later; no dependencies, no network.
import { deflateSync } from 'node:zlib';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const N = 1024;
const R = 230; // corner radius
const top = [0x4f, 0xa3, 0xff];
const bottom = [0x1a, 0x5f, 0xd6];

const raw = Buffer.alloc((N * 4 + 1) * N);
for (let y = 0; y < N; y++) {
  raw[y * (N * 4 + 1)] = 0; // filter: none
  const t = y / (N - 1);
  for (let x = 0; x < N; x++) {
    const cx = Math.min(Math.max(x, R), N - 1 - R);
    const cy = Math.min(Math.max(y, R), N - 1 - R);
    const inside = (x - cx) ** 2 + (y - cy) ** 2 <= R * R;
    const o = y * (N * 4 + 1) + 1 + x * 4;
    for (let c = 0; c < 3; c++) raw[o + c] = Math.round(top[c] + (bottom[c] - top[c]) * t);
    raw[o + 3] = inside ? 255 : 0;
  }
}

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
};
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(N, 0);
ihdr.writeUInt32BE(N, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // RGBA

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr),
  chunk('IDAT', deflateSync(raw)),
  chunk('IEND', Buffer.alloc(0)),
]);
writeFileSync(fileURLToPath(new URL('../src-tauri/app-icon.png', import.meta.url)), png);
