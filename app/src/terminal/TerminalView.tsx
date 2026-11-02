/** WP0 placeholder (WP4 mounts xterm.js here, keeps the export). The terminal surface uses `--term`. */
export function TerminalView() {
  return (
    <div
      className="flex-1 overflow-auto bg-term px-5 py-4 font-mono text-[13px] leading-normal whitespace-pre text-txt"
      aria-label="Terminal"
      data-testid="terminal-placeholder"
    >
      <span className="text-tdim">Terminal (WP4)</span>
    </div>
  );
}
