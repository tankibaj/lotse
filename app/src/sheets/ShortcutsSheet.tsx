import { closeOverlay } from './overlay';
import { Kbd, Modal } from './ui';

// Mock `#keys`, verbatim. Beta entries (editor, panels, split) are listed because the mock lists them.
export const SHORTCUTS: [string, string][] = [
  ['Command palette / jump anywhere', '⌘K'],
  ['Next session that needs you', '⌘J'],
  ['Switch to tab 1–9', '⌘1…9'],
  ['New session (Terminal by default)', '⌘T'],
  ['Open side by side (split)', '⌘D · ⌥-click · drag'],
  ['Focus previous / next pane', '⌘[ · ⌘]'],
  ['Full diff review', '⌥⌘D'],
  ['Close pane', '⌘W'],
  ['Open file in editor pane', 'Files panel · click'],
  ['Save file', '⌘S'],
  ['Expand / restore editor', '⌥⌘↩'],
  ['Toggle sidebar', '⌃⌘S'],
  ['Changes · Files · Transcript · Usage · Session', '⇧⌘D · ⇧⌘F · ⇧⌘T · ⇧⌘U · ⇧⌘I'],
  ['Add project folder', '⇧⌘O'],
  ['Settings', '⌘,'],
  ['Pause notifications', '⌥⌘N'],
  ['Terminal font bigger / smaller', '⌘+ / ⌘−'],
  ['This list', '⌘/'],
];

/** ⌘/ keyboard shortcuts sheet (mock `#keys`). */
export function ShortcutsSheet() {
  return (
    <Modal label="Keyboard shortcuts" onClose={closeOverlay} panelClass="relative mt-20 mb-8 w-[520px] rounded-xl p-5 text-[13px]">
      <button type="button" data-autofocus onClick={closeOverlay} className="absolute right-3 top-3 grid h-[26px] w-7 place-items-center rounded-md text-dim hover:bg-sel" aria-label="Close">✕</button>
      <h3 className="mb-3 font-semibold">Keyboard shortcuts</h3>
      <div className="grid grid-cols-[1fr_auto] gap-y-1.5">
        {SHORTCUTS.map(([what, keys]) => (
          <div key={what} className="contents">
            <span>{what}</span>
            <Kbd>{keys}</Kbd>
          </div>
        ))}
      </div>
    </Modal>
  );
}
