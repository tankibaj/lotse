import type { CSSProperties, ReactElement } from "react";
import { FONT_MONO, FONT_SANS, type Theme } from "../lib/theme";
import { FPS, easeInOut, easeIn, mix, ramp, spr } from "../lib/anim";
import { AGENT_LINES, lineColor } from "../lib/text";
import { StatusGlyph, type Status } from "../ui/StatusGlyph";

export const WX = 96, WY = 50, WW = 1728, WH = 800, SB = 340, TOP = 52, BAR = 44;

type Row = { id: string; title: string; agent: string };
type Project = { name: string; color: string; rows: Row[] };
const PROJECTS: Project[] = [
  { name: "checkout-service", color: "#5e5ce6", rows: [{ id: "a1", title: "Apply discount codes", agent: "Claude Code" }, { id: "a2", title: "Cart rounding", agent: "Codex" }] },
  { name: "payments-service", color: "#007aff", rows: [{ id: "a3", title: "Retry failed charges", agent: "Claude Code" }] },
  { name: "catalog-service", color: "#32ade6", rows: [{ id: "a4", title: "Variant pricing", agent: "OpenCode" }] },
  { name: "assistant-service", color: "#a2845e", rows: [{ id: "a5", title: "Assistant prompt evals", agent: "Claude Code" }] },
];

// Timeline (seconds)
export const T_CALLOUTS: [number, number] = [12.8, 15.3];
export const T_CARD_IN = 15.9, T_JUMP = 17.2, T_CLICK = 21.3, T_DONE = 22.4;

export const cursorPath = (frame: number) => {
  const t = frame / FPS;
  const p = ramp(frame, 19.8, 21.1, easeInOut);
  return { x: mix(1180, 194, p), y: mix(560, 279, p), visible: t >= 19.6 && t < 22.4, click: ramp(frame, T_CLICK, T_CLICK + 0.5) };
};

const planeShadow = (sep: number, dark: boolean): string =>
  sep < 0.02 ? "none" : `0 ${30 * sep}px ${60 * sep}px rgba(0,0,0,${(dark ? 0.55 : 0.2) * sep}), 0 ${6 * sep}px ${14 * sep}px rgba(0,0,0,${(dark ? 0.4 : 0.1) * sep})`;

export const LotseApp = ({ frame, T, intro, sep }: { frame: number; T: Theme; intro: number; sep: number }) => {
  const t = frame / FPS;
  const isLight = T.win === "#ffffff";
  const cardH = 100 * spr(frame, T_CARD_IN, 0.7) * (1 - spr(frame, T_DONE, 0.6));
  const allowed = t >= T_CLICK;
  const jumped = t >= T_JUMP;
  const a1Status: Status = t >= T_DONE ? "done" : allowed ? "run" : t >= 12.2 ? "perm" : "run";
  const status: Record<string, Status> = { a1: a1Status, a2: "run", a3: "run", a4: "done", a5: "run" };
  const spin = (frame * 6) % 360;
  const needs = a1Status === "perm" ? 1 : 0;
  const dark = !isLight;

  // selection highlight slides between rows (spring) instead of snapping
  let y = 14;
  const rowTop: Record<string, number> = {};
  const rowsOut: ReactElement[] = [];
  PROJECTS.forEach((p) => {
    const projStatus: Status = p.rows.some((r) => status[r.id] === "perm") ? "perm" : p.rows.some((r) => status[r.id] === "run") ? "run" : "done";
    rowsOut.push(
      <div key={p.name} style={{ position: "absolute", left: 18, top: y, width: SB - 36, height: 44, display: "flex", alignItems: "center", gap: 12 }}>
        <div style={{ width: 28, height: 28, borderRadius: 8, background: p.color, color: "#fff", fontFamily: FONT_SANS, fontWeight: 700, fontSize: 16, display: "grid", placeItems: "center", boxShadow: "0 2px 6px rgba(0,0,0,.18)" }}>{p.name[0].toUpperCase()}</div>
        <div style={{ flex: 1, fontFamily: FONT_SANS, fontWeight: 600, fontSize: 21, color: T.txt }}>{p.name}</div>
        <StatusGlyph status={projStatus} theme={T} size={20} spin={spin} />
      </div>,
    );
    y += 44;
    p.rows.forEach((r) => {
      rowTop[r.id] = y;
      rowsOut.push(
        <div key={r.id} style={{ position: "absolute", left: 12, top: y, width: SB - 24, height: 52, borderRadius: 10, display: "flex", alignItems: "center", gap: 12, padding: "0 12px 0 20px", boxSizing: "border-box" }}>
          <StatusGlyph status={status[r.id]} theme={T} size={20} spin={spin} />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontFamily: FONT_SANS, fontWeight: 500, fontSize: 19, color: T.txt, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{r.title}</div>
            <div style={{ fontFamily: FONT_SANS, fontSize: 15, color: T.dim }}>{r.agent}</div>
          </div>
        </div>,
      );
      y += 52;
      if (r.id === "a1" && cardH > 1) {
        rowsOut.push(
          <div key="card" style={{ position: "absolute", left: 18, top: y, width: SB - 36, height: cardH, overflow: "hidden", opacity: Math.min(1, cardH / 40) }}>
            <div style={{ margin: "4px 0 0", height: 92, borderRadius: 12, border: `1.5px solid ${T.orange}`, background: isLight ? "#fff8ec" : "#33281a", padding: "10px 14px", boxSizing: "border-box", boxShadow: `0 10px 26px rgba(255,149,0,${isLight ? 0.18 : 0.22})` }}>
              <div style={{ fontFamily: FONT_MONO, fontSize: 17, color: T.txt }}>Run <b>npm test</b>?</div>
              <div style={{ display: "flex", gap: 10, marginTop: 10 }}>
                <div style={{ flex: 1, height: 38, borderRadius: 9, background: T.blue, color: "#fff", fontFamily: FONT_SANS, fontWeight: 600, fontSize: 18, display: "grid", placeItems: "center", transform: `scale(${1 - 0.06 * (ramp(frame, T_CLICK, T_CLICK + 0.1, (x) => x) * (1 - ramp(frame, T_CLICK + 0.1, T_CLICK + 0.3, (x) => x)))})` }}>Allow</div>
                <div style={{ flex: 1, height: 38, borderRadius: 9, background: isLight ? "#ececf0" : "#3a3a3c", color: T.txt, fontFamily: FONT_SANS, fontWeight: 500, fontSize: 18, display: "grid", placeItems: "center" }}>Deny</div>
              </div>
            </div>
          </div>,
        );
        y += cardH;
      }
    });
    y += 6;
  });

  // sliding selection
  const selA = rowTop["a3"] ?? 0;
  const selB = rowTop["a1"] ?? 0;
  const selP = spr(frame, T_JUMP, 0.5, { damping: 20, stiffness: 160 });
  const selTop = jumped ? mix(selA, selB, selP) : selA;
  const selection = <div key="sel" style={{ position: "absolute", left: 12, top: selTop, width: SB - 24, height: 52, borderRadius: 10, background: T.sel }} />;

  const termLines: string[] = jumped
    ? ["✻ Apply discount codes", "● Update(src/cart/discount.ts)", "  ⎿ Updated with 18 additions, 6 removals", "● Bash(npm test)"]
    : Array.from({ length: Math.max(0, Math.floor(8 * (t - 9.6))) }, (_, k) => AGENT_LINES[(k + 7) % AGENT_LINES.length]).slice(-14);
  const showPrompt = jumped && !allowed;
  const swap = spr(frame, T_JUMP + 0.05, 0.6);
  const callIn = Math.min(spr(frame, T_CALLOUTS[0], 0.8), 1 - ramp(frame, T_CALLOUTS[1] - 0.5, T_CALLOUTS[1], easeIn));

  const plane = (z: number, extra: CSSProperties): CSSProperties => ({
    position: "absolute", transform: `translateZ(${z * sep}px)`, boxShadow: planeShadow(sep, dark), ...extra,
  });

  // window intro pose
  const k = 1 - intro;
  const pose = `translate3d(0, ${k * 170}px, ${-k * 520}px) rotateX(${k * 52}deg) rotateZ(${-k * 16}deg) scale(${1 - k * 0.1})`;

  return (
    <div style={{ position: "absolute", left: WX, top: WY, width: WW, height: WH, transformStyle: "preserve-3d", transform: pose }}>
      {/* backing with the big soft shadow */}
      <div style={{ position: "absolute", inset: 0, borderRadius: 20, background: T.win, transform: `translateZ(${-50 * sep}px)`, boxShadow: dark ? "0 2px 4px rgba(0,0,0,.4), 0 20px 40px rgba(0,0,0,.5), 0 60px 120px rgba(0,0,0,.6)" : "0 2px 4px rgba(0,0,0,.06), 0 18px 36px rgba(0,0,0,.10), 0 60px 120px rgba(0,0,0,.16)", outline: `1.5px solid ${dark ? "rgba(255,255,255,.07)" : "rgba(0,0,0,.06)"}` }} />

      {/* title bar */}
      <div style={plane(150, { left: 0, top: 0, width: WW, height: TOP, borderRadius: "20px 20px 0 0", background: T.win, borderBottom: `1.5px solid ${T.line}`, display: "flex", alignItems: "center", padding: "0 20px", gap: 9, boxSizing: "border-box" })}>
        {["#ff5f57", "#febc2e", "#28c840"].map((c) => <span key={c} style={{ width: 14, height: 14, borderRadius: 7, background: c }} />)}
        <div style={{ position: "absolute", left: SB + 30, top: 0, height: TOP, display: "flex", alignItems: "center", gap: 10, fontFamily: FONT_SANS, fontSize: 19 }}>
          <span style={{ fontWeight: 600, color: T.txt }}>{jumped ? "checkout-service" : "payments-service"}</span>
          <span style={{ color: T.dim }}>›  {jumped ? "Apply discount codes" : "Retry failed charges"}</span>
        </div>
        <div style={{ position: "absolute", right: 20, top: 9, width: 34, height: 34, borderRadius: 17, border: `1.5px solid ${T.line}`, display: "grid", placeItems: "center", color: T.dim }}>
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
            {isLight ? (<><circle cx="12" cy="12" r="4.2" /><path d="M12 2.5v2.4M12 19.1v2.4M2.5 12h2.4M19.1 12h2.4M5.3 5.3l1.7 1.7M17 17l1.7 1.7M5.3 18.7L7 17M17 7l1.7-1.7" /></>) : (<path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5z" />)}
          </svg>
        </div>
      </div>

      {/* sidebar */}
      <div style={plane(80, { left: 0, top: TOP, width: SB, height: WH - TOP - BAR, background: T.side, borderRight: `1.5px solid ${T.line}` })}>
        {selection}
        {rowsOut}
      </div>

      {/* terminal */}
      <div style={plane(0, { left: SB, top: TOP, right: 0, height: WH - TOP - BAR, background: T.term, padding: "22px 30px", boxSizing: "border-box", fontFamily: FONT_MONO, fontSize: 21, lineHeight: "32px", overflow: "hidden" })}>
        <div style={{ transform: `translateX(${jumped ? (1 - swap) * 40 : 0}px)`, opacity: jumped ? Math.min(1, swap * 1.5) : 1 }}>
          {termLines.map((l, i) => <div key={i} style={{ whiteSpace: "pre", color: lineColor(l, T.tdim, T.txt) }}>{l}</div>)}
          {showPrompt && (
            <div style={{ marginTop: 12, border: `1.5px solid ${T.line}`, borderRadius: 12, padding: "14px 20px", color: T.txt, boxShadow: "0 12px 30px rgba(0,0,0,.08)" }}>
              <div>Do you want to proceed?</div>
              <div style={{ color: T.blue }}>❯ 1. Yes</div>
              <div style={{ color: T.dim }}>  2. Yes, and don't ask again for npm test in checkout-service</div>
              <div style={{ color: T.dim }}>  3. No</div>
            </div>
          )}
          {allowed && (
            <>
              <div style={{ whiteSpace: "pre", color: T.tdim }}>  ⎿ 148 passed, 0 failed</div>
              <div style={{ color: T.txt, marginTop: 8 }}>● All checks pass. Ready for review.</div>
            </>
          )}
        </div>
        <div style={{ position: "absolute", inset: 0, background: isLight ? "rgba(255,255,255,.82)" : "rgba(30,30,30,.82)", opacity: callIn }} />
      </div>

      {/* status bar */}
      <div style={plane(200, { left: 0, top: WH - BAR, width: WW, height: BAR, borderRadius: "0 0 20px 20px", background: T.side, borderTop: `1.5px solid ${T.line}`, display: "flex", alignItems: "center", padding: "0 22px", gap: 26, fontFamily: FONT_SANS, fontSize: 18, color: T.dim, boxSizing: "border-box" })}>
        <span style={{ color: needs ? T.orange : T.dim, fontWeight: 600 }}>{needs} need{needs === 1 ? "s" : ""} you</span>
        <span>{Object.values(status).filter((v) => v === "run").length} running</span>
        <span>{Object.values(status).filter((v) => v === "done").length} done</span>
      </div>

      {/* infographic callouts: what each status means */}
      {callIn > 0.01 && (
        <>
          {([
            { y: 136, label: "Needs you", st: "perm" as Status, d: 0 },
            { y: 290, label: "Working", st: "run" as Status, d: 0.35 },
            { y: 392, label: "Done", st: "done" as Status, d: 0.7 },
          ]).map((c) => {
            const p = spr(frame, T_CALLOUTS[0] + 0.2 + c.d, 0.7, { damping: 16, stiffness: 140 });
            const exit = 1 - ramp(frame, T_CALLOUTS[1] - 0.5, T_CALLOUTS[1], easeIn);
            return (
              <div key={c.label} style={{ position: "absolute", left: 0, top: c.y - 34, width: 900, height: 68, opacity: Math.min(1, p * 2) * exit, }}>
                <div style={{ position: "absolute", left: SB - 6, top: 33, height: 3, width: 150 * p, background: T.blue, borderRadius: 2, opacity: 0.55 }} />
                <div style={{ position: "absolute", left: SB + 150 * p - 8, top: 29, width: 11, height: 11, borderRadius: 6, background: T.blue }} />
                <div style={{ position: "absolute", left: SB + 160, top: 0, height: 68, padding: "0 28px 0 22px", borderRadius: 34, background: isLight ? "#ffffff" : "#2c2c2e", display: "flex", alignItems: "center", gap: 16, fontFamily: FONT_SANS, fontWeight: 700, fontSize: 32, color: T.txt, boxShadow: "0 18px 40px rgba(0,0,0,.18), 0 2px 6px rgba(0,0,0,.08)", transform: `scale(${0.8 + 0.2 * p}) translateX(${(1 - p) * -30}px)`, transformOrigin: "left center" }}>
                  <StatusGlyph status={c.st} theme={T} size={32} spin={spin} />
                  {c.label}
                </div>
              </div>
            );
          })}
        </>
      )}
    </div>
  );
};
