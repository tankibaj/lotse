// Real Lotse tokens (app/src/theme/tokens.css), light and dark.
export type Theme = {
  win: string; side: string; line: string; txt: string; dim: string; term: string; tdim: string; sel: string;
  blue: string; orange: string; red: string; green: string; gray: string;
};

export const light: Theme = {
  win: "#ffffff", side: "#f5f5f7", line: "#e5e5ea", txt: "#1d1d1f", dim: "#6e6e73", term: "#ffffff", tdim: "#8a8a8e",
  sel: "rgba(0,122,255,0.12)", blue: "#007aff", orange: "#ff9500", red: "#ff3b30", green: "#34c759", gray: "#8e8e93",
};

export const dark: Theme = {
  win: "#1e1e1e", side: "#252527", line: "#38383a", txt: "#f5f5f7", dim: "#a1a1a6", term: "#1e1e1e", tdim: "#7c7c80",
  sel: "rgba(10,132,255,0.22)", blue: "#0a84ff", orange: "#ff9f0a", red: "#ff453a", green: "#30d158", gray: "#98989d",
};

export const FONT_SANS = "Inter, -apple-system, system-ui, sans-serif";
export const FONT_MONO = '"JetBrains Mono", ui-monospace, Menlo, monospace';
