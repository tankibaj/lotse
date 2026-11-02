import { emitUiCommand } from '../lib/ui/commands';

/** WP0 placeholder (WP3 replaces the body, keeps the export). Search field and Projects header as in the mock. */
export function Sidebar() {
  return (
    <>
      <div className="px-3">
        <div
          onClick={() => emitUiCommand('openPalette')}
          onKeyDown={(e) => e.key === 'Enter' && emitUiCommand('openPalette')}
          className="flex cursor-text items-center gap-2 rounded-md bg-line px-2 py-1.5"
          role="button"
          tabIndex={0}
          aria-label="Search, ⌘K"
        >
          <svg width="13" height="13" fill="none" stroke="currentColor" strokeWidth="1.6" className="text-dim" aria-hidden="true">
            <circle cx="5.5" cy="5.5" r="4" />
            <path d="M9 9l3 3" />
          </svg>
          <span className="text-dim">Search</span>
          <span className="ml-auto text-[11px] text-dim">⌘K</span>
        </div>
      </div>
      <div className="mt-4 mb-1 flex items-center px-4 text-[11px] font-semibold tracking-wide text-dim uppercase">
        Projects
        <button
          className="ml-auto grid h-[26px] w-7 place-items-center rounded-md hover:bg-sel"
          onClick={() => emitUiCommand('openAddProject')}
          aria-label="Add a project"
          title="Add a project ⇧⌘O"
        >
          +
        </button>
      </div>
      <nav className="min-h-0 flex-1 space-y-0.5 overflow-x-hidden overflow-y-auto overscroll-contain px-2 pb-3" aria-label="Projects" />
    </>
  );
}
