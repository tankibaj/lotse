import type { ReactNode } from "react";
import { FONT_SANS } from "../lib/theme";

/** Centered caption in the lower third. `show` 0..1 drives fade and a small rise. */
export const Caption = ({
  children, show, size = 96, color, bottom = 64, weight = 700,
}: { children: ReactNode; show: number; size?: number; color: string; bottom?: number; weight?: number }) => (
  <div
    style={{
      position: "absolute",
      left: 96,
      right: 96,
      bottom,
      textAlign: "center",
      fontFamily: FONT_SANS,
      fontWeight: weight,
      fontSize: size,
      lineHeight: 1.05,
      letterSpacing: "-0.03em",
      color,
      opacity: show,
      transform: `translateY(${(1 - show) * 24}px)`,
    }}
  >
    {children}
  </div>
);
