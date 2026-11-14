import type { ReactNode } from "react";
import type { Theme } from "../lib/theme";

export type Status = "run" | "perm" | "fail" | "done" | "idle";

// Same shapes as the app's StatusGlyph (app/src/sidebar/StatusGlyph.tsx).
const shapes = (t: Theme): Record<Status, ReactNode> => ({
  run: (
    <>
      <circle cx="8" cy="8" r="6" fill="none" stroke={t.line} strokeWidth="2" />
      <path d="M8 2a6 6 0 0 1 6 6" fill="none" stroke={t.blue} strokeWidth="2" strokeLinecap="round" />
    </>
  ),
  perm: (
    <>
      <path d="M8 2 14.6 13.6H1.4z" fill="none" stroke={t.orange} strokeWidth="1.5" strokeLinejoin="round" />
      <path d="M8 6.2v3.2" stroke={t.orange} strokeWidth="1.5" strokeLinecap="round" />
      <circle cx="8" cy="11.5" r=".85" fill={t.orange} />
    </>
  ),
  fail: (
    <>
      <circle cx="8" cy="8" r="7" fill={t.red} />
      <path d="M5.7 5.7l4.6 4.6M10.3 5.7l-4.6 4.6" stroke="#fff" strokeWidth="1.6" strokeLinecap="round" />
    </>
  ),
  done: (
    <>
      <circle cx="8" cy="8" r="6.25" fill="none" stroke={t.green} strokeWidth="1.5" />
      <path d="M5 8.2l2 2 4-4.2" fill="none" stroke={t.green} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
    </>
  ),
  idle: <circle cx="8" cy="8" r="6" fill="none" stroke={t.gray} strokeWidth="1.5" />,
});

export const StatusGlyph = ({ status, theme, size, spin = 0 }: { status: Status; theme: Theme; size: number; spin?: number }) => (
  <svg
    viewBox="0 0 16 16"
    width={size}
    height={size}
    style={{ display: "block", flex: "none", transform: status === "run" ? `rotate(${spin}deg)` : undefined }}
  >
    {shapes(theme)[status]}
  </svg>
);
