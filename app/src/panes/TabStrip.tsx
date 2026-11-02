import { emitUiCommand } from '../lib/ui/commands';

/** WP0 placeholder (WP4 replaces the body, keeps the export): one tab and the "+" button, as in the mock. */
export function TabStrip() {
  return (
    <>
      <div className="flex min-w-0 items-end gap-1">
        <div className="flex h-8 items-center gap-2 rounded-t-md border border-b-0 bg-term px-3 text-[12px]">
          <span className="max-w-[200px] truncate">Session</span>
          <span className="text-tdim">project</span>
        </div>
      </div>
      <button
        className="mb-2 grid h-[26px] w-7 place-items-center rounded-md text-dim hover:bg-sel"
        onClick={() => emitUiCommand('openNewSession')}
        aria-label="New session"
        title="New session ⌘T"
      >
        +
      </button>
    </>
  );
}
