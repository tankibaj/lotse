export const Cursor = ({ x, y, click }: { x: number; y: number; click: number }) => (
  <>
    <div
      style={{
        position: "absolute",
        left: x - 28,
        top: y - 28,
        width: 56,
        height: 56,
        borderRadius: 28,
        border: "3px solid #007aff",
        opacity: click > 0 && click < 1 ? 0.7 * (1 - click) : 0,
        transform: `scale(${0.4 + click * 1.1})`,
      }}
    />
    <svg
      width="44"
      height="44"
      viewBox="0 0 24 24"
      style={{ position: "absolute", left: x - 6, top: y - 4, transform: `scale(${1 - 0.12 * (click > 0 && click < 0.3 ? 1 : 0)})`, filter: "drop-shadow(0 4px 6px rgba(0,0,0,.3))" }}
    >
      <path d="M5 3l14 8-6.2 1.6L9.6 19z" fill="#1d1d1f" stroke="#fff" strokeWidth="1.6" strokeLinejoin="round" />
    </svg>
  </>
);
