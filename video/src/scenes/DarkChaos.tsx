import { FONT_MONO, FONT_SANS, light as T } from "../lib/theme";
import { FPS, camStyle, camera, easeOut, mix, ramp, spr } from "../lib/anim";
import { AGENT_LINES, lineColor, mmss } from "../lib/text";

// The problem scene (light theme): five agents in separate terminals, one is stalled.
const X0 = 96, Y0 = 90, W = 556, H = 300, GAP = 30;
const FOCUS_ORDER = [0, 1, 2, 4, 2, 0, 4, 1, 0, 2, 4, 1, 4, 0, 2, 1];
const SPEED = [9, 7, 11, 8, 10];
const STALLED = 3;
const STALL_AT = 1.2;
const STEP = 0.38;

export const stalledCenter = {
  x: X0 + (W + GAP) * (STALLED % 3) + W / 2,
  y: Y0 + (H + GAP) * Math.floor(STALLED / 3) + H / 2,
};

const shadow = "0 1px 2px rgba(0,0,0,.06), 0 10px 20px rgba(0,0,0,.08), 0 34px 70px rgba(0,0,0,.14)";

const Window = ({ i, t, frame }: { i: number; t: number; frame: number }) => {
  const col = i % 3, row = Math.floor(i / 3);
  const x = X0 + (W + GAP) * col, y = Y0 + (H + GAP) * row;
  const stalled = i === STALLED;
  const shown = Math.max(0, Math.floor(SPEED[i] * (Math.min(t, stalled ? STALL_AT : 99) - 0.2)));
  // 3D entrance, staggered
  const p = spr(frame, i * 0.06, 0.95, { damping: 18, stiffness: 110 });
  const side = col === 0 ? -1 : col === 2 ? 1 : 0;
  const idle = Math.sin(frame / 38 + i * 1.7);
  const step = t >= 5 ? Math.floor((t - 5) / STEP) : -1;
  const focused = step >= 0 && FOCUS_ORDER[step % FOCUS_ORDER.length] === i && t < 9;
  const local = step >= 0 ? ((t - 5) % STEP) / STEP : 0;
  const lines = Array.from({ length: shown }, (_, k) => AGENT_LINES[(k + i * 5) % AGENT_LINES.length]).slice(stalled && t > STALL_AT ? -6 : -9);
  const pulse = stalled && t > STALL_AT + 0.4 ? 0.5 + 0.5 * Math.sin((t - STALL_AT) * 5) : 0;
  return (
    <div
      style={{
        position: "absolute", left: x, top: y, width: W, height: H, borderRadius: 16, overflow: "hidden", background: T.term,
        opacity: Math.min(1, p * 1.5),
        transform: `translate3d(0, ${(1 - p) * 90 + idle * 3}px, ${-(1 - p) * 700}px) rotateY(${side * (1 - p) * 30}deg) rotateX(${(1 - p) * 16 + idle * 0.4}deg) scale(${focused ? 1.025 : 1})`,
        border: `2px solid ${focused ? T.blue : stalled && pulse > 0 ? `rgba(255,149,0,${0.2 + 0.5 * pulse})` : "rgba(0,0,0,.06)"}`,
        boxShadow: focused ? `0 0 0 5px ${T.sel}, ${shadow}` : shadow,
      }}
    >
      <div style={{ height: 38, display: "flex", alignItems: "center", gap: 8, padding: "0 14px", background: "#ececf0", borderBottom: `1px solid ${T.line}` }}>
        {["#ff5f57", "#febc2e", "#28c840"].map((c) => <span key={c} style={{ width: 12, height: 12, borderRadius: 6, background: c }} />)}
        <span style={{ marginLeft: 10, fontFamily: FONT_SANS, fontSize: 15, color: T.dim }}>zsh · agent {i + 1}</span>
      </div>
      <div style={{ padding: "14px 18px", fontFamily: FONT_MONO, fontSize: 17, lineHeight: "26px" }}>
        {lines.map((l, k) => <div key={k} style={{ color: lineColor(l, T.tdim, "#1d1d1f"), whiteSpace: "pre" }}>{l}</div>)}
        {stalled && t > STALL_AT && (
          <div style={{ marginTop: 6, color: "#1d1d1f" }}>
            <div>Allow Bash(npm test)?</div>
            <div style={{ color: T.tdim }}>  1. Yes   2. No <span style={{ opacity: Math.floor(frame / 30) % 2 ? 0 : 1, color: "#1d1d1f" }}>█</span></div>
          </div>
        )}
      </div>
      {focused && (
        <div style={{ position: "absolute", right: 14, top: 48, width: 46, height: 46, borderRadius: 23, background: T.blue, color: "#fff", fontFamily: FONT_SANS, fontWeight: 700, fontSize: 30, display: "grid", placeItems: "center", transform: `scale(${Math.min(1, local * 5)})`, boxShadow: "0 8px 20px rgba(0,122,255,.35)" }}>?</div>
      )}
    </div>
  );
};

export const DarkChaos = ({ frame }: { frame: number }) => {
  const t = frame / FPS;
  const sec = mix(19 * 60 + 40, 20 * 60 + 34, ramp(frame, 0.0, 9, (x) => x));
  const clockFade = spr(frame, 0.2, 0.8);
  const cam = camera(frame, [[0, 1, 960, 540], [9.2, 1.05, 960, 520]]);
  const fill = Math.min(1, (sec - 600) / 900); // 10 min..25 min
  return (
    <div style={{ position: "absolute", inset: 0, perspective: 2200, perspectiveOrigin: "50% 45%" }}>
      <div style={{ position: "absolute", inset: 0, ...camStyle(cam), transformStyle: "preserve-3d" }}>
        {[0, 1, 2, 3, 4].map((i) => <Window key={i} i={i} t={t} frame={frame} />)}
        <div
          style={{
            position: "absolute", left: X0 + (W + GAP) * 2, top: Y0 + H + GAP, width: W, height: H, borderRadius: 16, background: "#ffffff",
            boxShadow: shadow, border: "2px solid rgba(0,0,0,.06)", opacity: clockFade, transform: `translateY(${(1 - clockFade) * 40}px)`,
            display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 6,
          }}
        >
          <div style={{ fontFamily: FONT_SANS, fontSize: 26, fontWeight: 600, color: T.dim, letterSpacing: "0.06em", textTransform: "uppercase" }}>agent waiting</div>
          <div style={{ fontFamily: FONT_MONO, fontSize: 138, fontWeight: 500, color: T.orange, letterSpacing: "-0.04em", lineHeight: 1.05 }}>{mmss(sec)}</div>
          <div style={{ width: 420, marginTop: 6 }}>
            <div style={{ height: 10, borderRadius: 5, background: "#ececf0", overflow: "hidden" }}>
              <div style={{ width: `${fill * 100}%`, height: "100%", background: `linear-gradient(90deg, ${T.orange}, #ff5e3a)`, borderRadius: 5 }} />
            </div>
            <div style={{ display: "flex", justifyContent: "space-between", marginTop: 8, fontFamily: FONT_SANS, fontSize: 18, color: T.tdim }}>
              <span>10 min</span><span>20 min</span><span>25 min</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
