import { test } from '@playwright/test';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

// Run after parity.spec.ts: writes parity/polish/diff-<size>-<scheme>[-<surface>].png (mock red, app blue, equal = faded)
// and logs the share of differing pixels in the chrome regions (sidebar header, tab row, status bar), where
// the mock and the app show the same content. The terminal text and the sidebar session list differ by data.

const HERE = fileURLToPath(new URL('.', import.meta.url));
const DIR = resolve(HERE, '../../../parity/polish');
const png = (f: string) => `data:image/png;base64,${readFileSync(`${DIR}/${f}`).toString('base64')}`;

const REGIONS: Record<string, [number, number, number, number]> = {
  'side-head': [0, 0, 260, 90],
  'tab-row': [260, 0, 1000, 44],
  'status-bar': [0, 872, 700, 28],
  terminal: [270, 55, 620, 520],
};

const ALL = ['', '-settings', '-palette', '-new', '-add'];
for (const { tag, cssWidth, surfaces } of [
  { tag: '1440', cssWidth: 1440, surfaces: ALL },
  { tag: '1280', cssWidth: 1280, surfaces: ALL },
  { tag: '1440@2x', cssWidth: 1440, surfaces: [''] },
]) {
  for (const scheme of ['light', 'dark']) {
    for (const surface of surfaces) {
      const name = `${tag}-${scheme}${surface}`;
      test(`diff ${name}`, async ({ page }) => {
        test.skip(!existsSync(`${DIR}/mock-${name}.png`) || !existsSync(`${DIR}/app-${name}.png`), 'no shots');
        await page.setContent('<canvas id="c"></canvas>');
        const res = await page.evaluate(
          async ({ a, b, regions: cssRegions, out, cssWidth }) => {
            const load = (src: string) =>
              new Promise<HTMLImageElement>((ok) => {
                const i = new Image();
                i.onload = () => ok(i);
                i.src = src;
              });
            const [ia, ib] = await Promise.all([load(a), load(b)]);
            const scale = ia.width / cssWidth; // 2 for the Retina shots; the regions are in css px
            const regions = Object.fromEntries(Object.entries(cssRegions).map(([k, r]) => [k, r.map((n) => n * scale)]));
            const W = Math.min(ia.width, ib.width);
            const H = Math.min(ia.height, ib.height);
            const c = document.getElementById('c') as HTMLCanvasElement;
            c.width = W;
            c.height = H;
            const ctx = c.getContext('2d', { willReadFrequently: true })!;
            ctx.drawImage(ia, 0, 0);
            const da = ctx.getImageData(0, 0, W, H);
            ctx.clearRect(0, 0, W, H);
            ctx.drawImage(ib, 0, 0);
            const db = ctx.getImageData(0, 0, W, H);
            const o = ctx.createImageData(W, H);
            const bad: Record<string, number> = {};
            for (const k of Object.keys(regions)) bad[k] = 0;
            const at = (d: ImageData, i: number) => d.data[i] ?? 0;
            const lumAt = (d: ImageData, i: number) => (at(d, i) + at(d, i + 1) + at(d, i + 2)) / 3;
            for (let y = 0; y < H; y++)
              for (let x = 0; x < W; x++) {
                const i = (y * W + x) * 4;
                const d = Math.abs(at(da, i) - at(db, i)) + Math.abs(at(da, i + 1) - at(db, i + 1)) + Math.abs(at(da, i + 2) - at(db, i + 2));
                if (d <= 24) {
                  const v = 200 + lumAt(da, i) / 6;
                  o.data.set([v, v, v, 255], i);
                } else {
                  const mockDarker = lumAt(da, i) < lumAt(db, i); // red where the mock is darker, blue where the app is
                  o.data.set([mockDarker ? 255 : 0, 0, mockDarker ? 0 : 255, 255], i);
                  for (const [k, [rx = 0, ry = 0, rw = 0, rh = 0]] of Object.entries(regions)) {
                    if (x >= rx && x < rx + rw && y >= ry && y < ry + rh) bad[k] = (bad[k] ?? 0) + 1;
                  }
                }
              }
            ctx.putImageData(o, 0, 0);
            const stats = Object.fromEntries(Object.entries(regions).map(([k, [, , rw = 1, rh = 1]]) => [k, +(((bad[k] ?? 0) / (rw * rh)) * 100).toFixed(2)]));
            return { stats, url: out ? c.toDataURL('image/png') : '' };
          },
          { a: png(`mock-${name}.png`), b: png(`app-${name}.png`), regions: REGIONS, out: true, cssWidth },
        );
        writeFileSync(`${DIR}/diff-${name}.png`, Buffer.from(res.url.split(',')[1] ?? '', 'base64'));
        console.log(name, JSON.stringify(res.stats));
      });
    }
  }
}
