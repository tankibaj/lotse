import { FONT_SANS } from "../lib/theme";

export const Keycap = ({ label, pressed, dark }: { label: string; pressed: number; dark: boolean }) => (
  <span
    style={{
      display: "inline-flex",
      alignItems: "center",
      justifyContent: "center",
      minWidth: 88,
      height: 88,
      padding: "0 22px",
      borderRadius: 20,
      fontFamily: FONT_SANS,
      fontWeight: 600,
      fontSize: 48,
      color: dark ? "#f5f5f7" : "#1d1d1f",
      background: dark ? "#2c2c2e" : "#ffffff",
      border: `2px solid ${dark ? "#48484a" : "#d2d2d7"}`,
      boxShadow: `0 ${6 - 5 * pressed}px 0 ${dark ? "#161617" : "#c7c7cc"}, 0 14px 30px rgba(0,0,0,${0.10 - 0.04 * pressed})`,
      transform: `translateY(${5 * pressed}px)`,
    }}
  >
    {label}
  </span>
);
