import { FONT_SANS, light as T } from "../lib/theme";
import { LotseIcon } from "./Icon";

export const Notification = ({ y, opacity }: { y: number; opacity: number }) => (
  <div
    style={{
      position: "absolute", left: 1324, top: y, width: 500, borderRadius: 24, background: "#ffffff", opacity,
      boxShadow: "0 24px 60px rgba(0,0,0,.22), 0 0 0 1.5px rgba(0,0,0,.06)", padding: "18px 22px", boxSizing: "border-box",
      display: "flex", gap: 18, alignItems: "flex-start", fontFamily: FONT_SANS,
    }}
  >
    <LotseIcon size={52} />
    <div style={{ flex: 1 }}>
      <div style={{ fontWeight: 600, fontSize: 21, color: T.txt }}>checkout-service · Claude Code</div>
      <div style={{ fontSize: 20, color: T.dim, marginTop: 2 }}>Needs permission: npm test</div>
      <div style={{ display: "flex", gap: 22, marginTop: 10, fontWeight: 600, fontSize: 19, color: T.blue }}>
        <span>Allow</span><span>Deny</span><span>Always allow</span>
      </div>
    </div>
  </div>
);
