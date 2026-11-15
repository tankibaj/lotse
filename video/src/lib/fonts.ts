import { staticFile } from "remotion";
import { loadFont } from "@remotion/fonts";

const files: [string, string, string][] = [
  ["Inter", "fonts/inter-latin-400-normal.woff2", "400"],
  ["Inter", "fonts/inter-latin-500-normal.woff2", "500"],
  ["Inter", "fonts/inter-latin-600-normal.woff2", "600"],
  ["Inter", "fonts/inter-latin-700-normal.woff2", "700"],
  ["JetBrains Mono", "fonts/jetbrains-mono-latin-400-normal.woff2", "400"],
  ["JetBrains Mono", "fonts/jetbrains-mono-latin-500-normal.woff2", "500"],
];

export const fontsReady = Promise.all(
  files.map(([family, path, weight]) => loadFont({ family, url: staticFile(path), weight })),
);
