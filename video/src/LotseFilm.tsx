import { AbsoluteFill, useCurrentFrame, delayRender, continueRender } from "remotion";
import { useEffect, useState } from "react";
import { fontsReady } from "./lib/fonts";
import { FPS, camStyle, camera, easeIn, easeInOut, easeOut, mix, ramp, spr, type Key } from "./lib/anim";
import { dark, light, FONT_SANS } from "./lib/theme";
import { DarkChaos, stalledCenter } from "./scenes/DarkChaos";
import { LotseApp, cursorPath } from "./scenes/LotseApp";
import { EndCard } from "./scenes/EndCard";
import { Backdrop } from "./ui/Backdrop";
import { wordCaption } from "./ui/WordCaption";
import { Keycap } from "./ui/Keycap";
import { Cursor } from "./ui/Cursor";
import { Notification } from "./ui/Notification";
import { Sounds } from "./ui/Sounds";

const TOGGLE = { x: 1776, y: 76 };
const T_TOGGLE_CLICK = 24.3;

const CAM: Key[] = [
  [9.5, 1.14, 960, 610], [11.8, 1, 960, 540], [12.8, 1, 960, 540],
  [13.7, 1.5, 430, 330], [15.0, 1.5, 430, 330], [15.9, 1, 960, 540], [16.9, 1, 960, 540],
  [17.7, 1.22, 640, 420], [18.8, 1.22, 640, 420], [19.6, 1, 960, 540],
  [20.4, 1.55, 360, 300], [22.0, 1.55, 360, 300], [23.0, 1, 960, 540],
  [24.4, 1, 960, 540], [26.4, 1.05, 960, 520],
];

/** The product scene, rendered once per appearance. */
const Product = ({ frame, mode }: { frame: number; mode: "light" | "dark" }) => {
  const isLight = mode === "light";
  const T = isLight ? light : dark;
  const intro = spr(frame, 9.5, 1.9, { damping: 20, stiffness: 55 });
  const sep = 1 - spr(frame, 10.6, 1.5);
  const cam = camera(frame, CAM);
  const cur = cursorPath(frame);
  const t = frame / FPS;
  const cur2p = ramp(frame, 23.3, 24.2, easeInOut);
  const cur2 = { x: mix(1000, TOGGLE.x, cur2p), y: mix(520, TOGGLE.y, cur2p), visible: t >= 23.2 && t < 25.2, click: ramp(frame, T_TOGGLE_CLICK, T_TOGGLE_CLICK + 0.5) };
  const keyPop = (d: number) => spr(frame, 17.0 + d, 0.5, { damping: 14, stiffness: 160 });
  const jumpKey = ramp(frame, 17.15, 17.3, (x) => x) * (1 - ramp(frame, 17.5, 17.7, (x) => x));
  const c = (text: string, a: number, b: number, size = 92, extra = {}) => wordCaption(frame, { text, start: a, end: b, size, color: T.txt, bottom: 84, ...extra });
  return (
    <>
      <Backdrop frame={frame} mode={mode} />
      <div style={{ position: "absolute", inset: 0, ...camStyle(cam) }}>
        <div style={{ position: "absolute", inset: 0, perspective: 2600, perspectiveOrigin: "50% 55%" }}>
          <LotseApp frame={frame} T={T} intro={intro} sep={sep} />
        </div>
        <Notification y={mix(-170, 70, ramp(frame, 15.5, 16.1, easeOut))} opacity={Math.min(ramp(frame, 15.5, 15.8, (x) => x), 1 - ramp(frame, 17.5, 17.9, easeIn))} />
        {cur.visible && <Cursor x={cur.x} y={cur.y} click={cur.click} />}
        {cur2.visible && <Cursor x={cur2.x} y={cur2.y} click={cur2.click} />}
      </div>
      <div style={{ position: "absolute", left: 0, right: 0, bottom: 0, height: 340, opacity: Math.min(1, Math.max(0, (cam.s - 1.1) / 0.25)), background: isLight ? "linear-gradient(to top, rgba(240,240,244,.96) 0%, rgba(240,240,244,.85) 45%, rgba(240,240,244,0) 100%)" : "linear-gradient(to top, rgba(11,11,12,.96) 0%, rgba(11,11,12,.85) 45%, rgba(11,11,12,0) 100%)" }} />
      {c("Meet Lotse.", 10.3, 12.1, 140)}
      {c("See who needs you.", 12.7, 15.4)}
      {wordCaption(frame, {
        text: "Jump there.", start: 17.0, end: 19.9, size: 80, color: T.txt, bottom: 56,
        suffix: (
          <div style={{ display: "flex", justifyContent: "center", gap: 14, marginTop: 8 }}>
            <div style={{ transform: `scale(${keyPop(0.1)})` }}><Keycap label="⌘" pressed={jumpKey} dark={!isLight} /></div>
            <div style={{ transform: `scale(${keyPop(0.2)})` }}><Keycap label="J" pressed={jumpKey} dark={!isLight} /></div>
          </div>
        ),
      })}
      {c("Allow.", 20.3, 23.9, 110)}
      {c("Light or dark.", 24.9, 26.7)}
    </>
  );
};

export const LotseFilm = ({ showRepo }: { showRepo: boolean }) => {
  const frame = useCurrentFrame();
  const t = frame / FPS;
  const [handle] = useState(() => delayRender("fonts"));
  useEffect(() => {
    fontsReady.then(() => continueRender(handle)).catch(() => continueRender(handle));
  }, [handle]);

  const reveal = ramp(frame, 9.0, 10.1, (x) => 1 - Math.pow(1 - x, 4));
  const clip = `circle(${mix(0, 2400, reveal)}px at ${stalledCenter.x}px ${stalledCenter.y}px)`;
  const flip = ramp(frame, T_TOGGLE_CLICK + 0.05, T_TOGGLE_CLICK + 1.05, (x) => 1 - Math.pow(1 - x, 4));
  const flipClip = `circle(${mix(0, 2300, flip)}px at ${TOGGLE.x}px ${TOGGLE.y}px)`;
  const appOut = ramp(frame, 26.5, 27.2, easeIn);
  const endIn = t >= 26.9;
  const p = { color: light.txt, bottom: 84, accent: light.orange };

  return (
    <AbsoluteFill style={{ fontFamily: FONT_SANS }}>
      <Sounds />
      <Backdrop frame={frame} mode="light" />
      <DarkChaos frame={frame} />
      {wordCaption(frame, { ...p, text: "You run five agents.", start: 0.0, end: 2.3, size: 120 })}
      {wordCaption(frame, { ...p, text: "One has been waiting\n20 minutes.", start: 2.5, end: 4.8, size: 112, accentWords: ["20", "minutes"] })}
      {wordCaption(frame, { ...p, text: "You don't know which.", start: 5.0, end: 8.9, size: 120, accentWords: ["which"] })}

      <div style={{ position: "absolute", inset: 0, clipPath: clip }}>
        <div style={{ position: "absolute", inset: 0, opacity: 1 - appOut, transform: `scale(${mix(1, 0.94, appOut)})` }}>
          <Product frame={frame} mode="light" />
          {flip > 0 && (
            <div style={{ position: "absolute", inset: 0, clipPath: flipClip }}>
              <Product frame={frame} mode="dark" />
            </div>
          )}
        </div>
        {endIn && <div style={{ position: "absolute", inset: 0, opacity: ramp(frame, 26.9, 27.5) }}><EndCard frame={frame} showRepo={showRepo} /></div>}
      </div>

      <AbsoluteFill style={{ background: "#f2f2f4", opacity: ramp(frame, 30.7, 31.0) }} />
    </AbsoluteFill>
  );
};
