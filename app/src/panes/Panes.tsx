import { TerminalView } from '../terminal';

/** WP0 placeholder (WP4 replaces the body, keeps the export): one pane filling the area. */
export function Panes() {
  return (
    <section className="flex min-w-0 flex-1 flex-col">
      <TerminalView />
    </section>
  );
}
