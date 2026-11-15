// Placeholder agent output (no real project, client or employer names).
export const AGENT_LINES = [
  "✻ Reading src/cart/discount.ts",
  "● Update(src/checkout/totals.ts)",
  "  ⎿ Updated with 12 additions, 3 removals",
  "● Bash(npm run lint)",
  "  ⎿ no problems found",
  "✻ Planning the refactor…",
  "● Read(src/payments/retry.ts)",
  "● Update(src/payments/charge.ts)",
  "  ⎿ Updated with 4 additions",
  "● Bash(go test ./catalog/...)",
  "  ⎿ ok  catalog/pricing  0.42s",
  "✻ Thinking…",
  "● Update(src/cart/discount.ts)",
  "  ⎿ Updated with 18 additions, 6 removals",
  "● Read(src/assistant/prompts.ts)",
  "✻ Checking the types…",
  "● Bash(npx tsc --noEmit)",
  "  ⎿ no errors",
  "● Update(evals/assistant.spec.ts)",
  "  ⎿ Updated with 9 additions",
];

export const lineColor = (l: string, dim: string, fg: string) =>
  l.startsWith("  ⎿") ? dim : l.startsWith("✻") ? "#c28a1c" : fg;

export const mmss = (sec: number) => {
  const m = Math.floor(sec / 60);
  const r = Math.floor(sec % 60);
  return `${String(m).padStart(2, "0")}:${String(r).padStart(2, "0")}`;
};
