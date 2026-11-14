import { FPS } from "../lib/anim";

/** Depth backdrop: soft studio gradient with a slow drift and a vignette (no flat fill). */
export const Backdrop = ({ frame, mode }: { frame: number; mode: "light" | "dark" }) => {
  const t = frame / FPS;
  const dx = Math.sin(t * 0.35) * 60;
  const dy = Math.cos(t * 0.27) * 30;
  const light = mode === "light";
  return (
    <div style={{ position: "absolute", inset: 0, overflow: "hidden", background: light ? "#e9e9ee" : "#0b0b0c" }}>
      <div
        style={{
          position: "absolute", inset: -200,
          background: light
            ? `radial-gradient(1500px 950px at ${50 + dx / 40}% ${40 + dy / 30}%, #ffffff 0%, #f4f4f7 45%, #e3e3e9 100%)`
            : `radial-gradient(1500px 950px at ${50 + dx / 40}% ${40 + dy / 30}%, #26262a 0%, #151517 50%, #0b0b0c 100%)`,
        }}
      />
      <div
        style={{
          position: "absolute", inset: 0,
          background: light
            ? "radial-gradient(ellipse at 50% 120%, rgba(0,0,0,.10), transparent 55%)"
            : "radial-gradient(ellipse at 50% 120%, rgba(0,0,0,.55), transparent 55%)",
        }}
      />
    </div>
  );
};
