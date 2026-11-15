import { Easing, interpolate, spring } from "remotion";

export const FPS = 60;
export const WIDTH = 1920;
export const HEIGHT = 1080;
export const SECONDS = 31;
export const TOTAL_FRAMES = FPS * SECONDS;

/** Seconds to frames. */
export const s = (sec: number) => Math.round(sec * FPS);

// One motion language: expo-out for entrances, standard in-out for moves, accelerate for exits.
export const easeOut = Easing.bezier(0.16, 1, 0.3, 1);
export const easeInOut = Easing.bezier(0.65, 0, 0.35, 1);
export const easeIn = Easing.bezier(0.4, 0, 1, 1);

/** 0..1 progress between two seconds, clamped, with an easing. */
export const ramp = (
  frame: number,
  from: number,
  to: number,
  easing: (t: number) => number = easeOut,
) =>
  interpolate(frame, [s(from), s(to)], [0, 1], {
    easing,
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

/** Linear mix. */
export const mix = (a: number, b: number, t: number) => a + (b - a) * t;

/** Spring progress 0..1 starting at `startSec`. Smooth (no bounce) unless a config is given. */
export const spr = (
  frame: number,
  startSec: number,
  durSec = 1.2,
  config: { damping?: number; stiffness?: number; mass?: number } = { damping: 200 },
) =>
  spring({ frame: frame - s(startSec), fps: FPS, config, durationInFrames: s(durSec) });

/** Camera keyframes: [sec, scale, centerX, centerY]; eased between keys. */
export type Key = [number, number, number, number];
export const camera = (frame: number, keys: Key[]) => {
  const t = frame / FPS;
  let a = keys[0];
  let b = keys[keys.length - 1];
  for (let i = 0; i < keys.length - 1; i++) {
    if (t >= keys[i][0] && t <= keys[i + 1][0]) { a = keys[i]; b = keys[i + 1]; break; }
  }
  if (t <= keys[0][0]) return { s: keys[0][1], cx: keys[0][2], cy: keys[0][3] };
  if (t >= keys[keys.length - 1][0]) { const k = keys[keys.length - 1]; return { s: k[1], cx: k[2], cy: k[3] }; }
  const p = easeInOut((t - a[0]) / (b[0] - a[0]));
  return { s: mix(a[1], b[1], p), cx: mix(a[2], b[2], p), cy: mix(a[3], b[3], p) };
};
export const camStyle = (c: { s: number; cx: number; cy: number }) => ({
  transformOrigin: "0 0",
  transform: `translate(${960 - c.cx * c.s}px, ${540 - c.cy * c.s}px) scale(${c.s})`,
});
