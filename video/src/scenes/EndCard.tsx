import { FONT_MONO, FONT_SANS, light as T } from "../lib/theme";
import { easeOut, ramp } from "../lib/anim";
import { LotseIcon } from "../ui/Icon";

export const EndCard = ({ frame, showRepo }: { frame: number; showRepo: boolean }) => {
  const a = ramp(frame, 27.2, 28.0);
  const b = ramp(frame, 27.7, 28.5);
  const c = ramp(frame, 28.1, 28.8);
  return (
    <div style={{ position: "absolute", inset: 0, background: "#f2f2f4", display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center" }}>
      <div style={{ opacity: a, transform: `scale(${0.9 + 0.1 * easeOut(a)})` }}><LotseIcon size={170} /></div>
      <div style={{ marginTop: 34, fontFamily: FONT_SANS, fontWeight: 700, fontSize: 132, letterSpacing: "-0.04em", color: T.txt, opacity: b, transform: `translateY(${(1 - b) * 20}px)` }}>Lotse</div>
      <div style={{ marginTop: 6, fontFamily: FONT_SANS, fontWeight: 500, fontSize: 84, letterSpacing: "-0.03em", color: T.txt, opacity: c, transform: `translateY(${(1 - c) * 16}px)` }}>Know who’s waiting.</div>
      <div style={{ marginTop: 22, fontFamily: FONT_SANS, fontWeight: 500, fontSize: 40, color: T.dim, opacity: c }}>macOS · open source</div>
      {showRepo && (
        <div style={{ position: "absolute", bottom: 56, fontFamily: FONT_MONO, fontSize: 52, color: T.txt, opacity: c, background: "#fff", padding: "14px 36px", borderRadius: 44, boxShadow: "0 1px 2px rgba(0,0,0,.06), 0 12px 30px rgba(0,0,0,.10)", border: "1px solid rgba(0,0,0,.06)" }}>github.com/tankibaj/lotse</div>
      )}
    </div>
  );
};
