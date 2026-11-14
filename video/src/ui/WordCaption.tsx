import type { ReactNode } from "react";
import { FONT_SANS } from "../lib/theme";
import { FPS, easeIn, ramp, spr } from "../lib/anim";

type Props = {
  text: string; start: number; end: number; size?: number; color: string; accent?: string; accentWords?: string[]; bottom?: number; suffix?: ReactNode;
};

/** Caption that rises in word by word (spring), then fades out. Lines split on "\n". Call as a function with the frame. */
export const wordCaption = (frame: number, { text, start, end, size = 96, color, accent, accentWords = [], bottom = 90, suffix }: Props) => {
  if (frame < start * FPS - 2 || frame > end * FPS + 2) return null;
  const out = 1 - ramp(frame, end - 0.35, end, easeIn);
  let n = 0;
  const lines = text.split("\n");
  return (
    <div
      style={{
        position: "absolute", left: 96, right: 96, bottom, textAlign: "center", fontFamily: FONT_SANS, fontWeight: 700,
        fontSize: size, lineHeight: 1.06, letterSpacing: "-0.035em", color, opacity: out,
      }}
    >
      {lines.map((line, li) => (
        <div key={li}>
          {line.split(" ").map((w, wi) => {
            const p = spr(frame, start + n * 0.09, 0.7);
            n += 1;
            const isAccent = accent && accentWords.some((a) => w.replace(/[.,]/g, "") === a);
            return (
              <span
                key={wi}
                style={{
                  display: "inline-block", marginRight: "0.26em", opacity: Math.min(1, p * 1.6),
                  transform: `translateY(${(1 - p) * 38}px)`, color: isAccent ? accent : color,
                }}
              >
                {w}
              </span>
            );
          })}
        </div>
      ))}
      {suffix}
    </div>
  );
};
