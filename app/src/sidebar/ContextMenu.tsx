import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from 'react';
import { createPortal } from 'react-dom';

// macOS-style context menu from the mock: 240 px box, blue highlight, one submenu level (Notifications),
// Esc / click outside closes, ↑↓ move, → opens a submenu, ← closes it. Unavailable items stay visible, dimmed.

export type MenuEntry =
  | { kind: 'header'; label: string }
  | { kind: 'separator' }
  | { kind: 'note'; label: string }
  | {
      kind: 'item';
      label: ReactNode;
      /** Right-aligned key hint, or the reason it is unavailable ("Beta"). */
      hint?: string;
      /** Small glyph in the leading column (instead of the ✓ slot). */
      icon?: ReactNode;
      disabled?: boolean;
      checked?: boolean;
      danger?: boolean;
      /** Stay open after choosing (toggles inside a submenu). */
      keepOpen?: boolean;
      onSelect?: () => void;
      submenu?: MenuEntry[];
    };

const BOX =
  'absolute rounded-lg p-1 text-[13px] text-txt shadow-2xl ring-1 ring-black/10 backdrop-blur-xl dark:ring-white/15 [background:color-mix(in_srgb,var(--win)_88%,transparent)]';

const items = (root: HTMLElement | null): HTMLElement[] =>
  root ? [...root.querySelectorAll<HTMLElement>('[role^="menuitem"]:not([aria-disabled="true"])')] : [];

function Entries({ entries, onHoverItem, onPick }: {
  entries: MenuEntry[];
  onHoverItem: (index: number, el: HTMLElement) => void;
  onPick: (entry: Extract<MenuEntry, { kind: 'item' }>) => void;
}) {
  return (
    <>
      {entries.map((e, i) => {
        if (e.kind === 'header') {
          return (
            <div key={i} className="px-3 pt-1 pb-1.5 text-[11px] font-semibold text-dim">
              {e.label}
            </div>
          );
        }
        if (e.kind === 'separator') return <div key={i} role="separator" className="mx-2 my-1 border-t" />;
        if (e.kind === 'note') {
          return (
            <div key={i} className="px-3 py-1 text-[11px] text-dim">
              {e.label}
            </div>
          );
        }
        return (
          <button
            key={i}
            type="button"
            role={e.checked === undefined ? 'menuitem' : 'menuitemcheckbox'}
            aria-checked={e.checked === undefined ? undefined : e.checked}
            aria-disabled={e.disabled || undefined}
            aria-haspopup={e.submenu ? 'menu' : undefined}
            tabIndex={-1}
            data-entry={i}
            onMouseEnter={(ev) => onHoverItem(i, ev.currentTarget)}
            onClick={() => !e.disabled && !e.submenu && onPick(e)}
            className={`flex w-full items-center gap-2 rounded-md px-2 py-1 text-left outline-none ${
              e.disabled
                ? 'opacity-40'
                : `cursor-default hover:bg-blue hover:text-white focus:bg-blue focus:text-white ${e.danger ? 'text-red' : ''}`
            }`}
          >
            <span className={e.icon ? 'grid w-4 place-items-center' : 'w-3 text-center'} aria-hidden="true">
              {e.icon ?? (e.checked ? '✓' : '')}
            </span>
            <span className="flex-1">{e.label}</span>
            <span className="text-[12px] opacity-60">{e.submenu ? '›' : e.hint}</span>
          </button>
        );
      })}
    </>
  );
}

export function ContextMenu({ x, y, label, entries, onClose }: {
  x: number;
  y: number;
  label: string;
  entries: MenuEntry[];
  onClose: () => void;
}) {
  const mainRef = useRef<HTMLDivElement>(null);
  const subRef = useRef<HTMLDivElement>(null);
  const opener = useRef<Element | null>(document.activeElement);
  const [pos, setPos] = useState({ left: x, top: y });
  const [sub, setSub] = useState<{ index: number; left: number; top: number } | null>(null);

  useLayoutEffect(() => {
    const r = mainRef.current?.getBoundingClientRect();
    if (r) setPos({ left: Math.max(4, Math.min(x, innerWidth - r.width - 10)), top: Math.max(4, Math.min(y, innerHeight - r.height - 8)) });
  }, [x, y]);

  useEffect(() => {
    items(mainRef.current)[0]?.focus();
    const from = opener.current;
    return () => {
      // Give focus back to where it was, unless something else (a rename field) took it.
      if (from instanceof HTMLElement && from.isConnected && document.activeElement === document.body) from.focus();
    };
  }, []);

  const openSub = (index: number, el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    const box = mainRef.current?.getBoundingClientRect();
    const right = (box?.right ?? r.right) + 262 < innerWidth;
    setSub({ index, left: right ? (box?.right ?? r.right) + 2 : (box?.left ?? r.left) - 262, top: Math.max(4, Math.min(r.top - 4, innerHeight - 220)) });
  };

  const pick = (e: Extract<MenuEntry, { kind: 'item' }>) => {
    if (!e.keepOpen) onClose();
    e.onSelect?.();
  };

  const onKeyDown = (ev: ReactKeyboardEvent) => {
    const inSub = subRef.current?.contains(document.activeElement) ?? false;
    const list = items(inSub ? subRef.current : mainRef.current);
    const i = list.indexOf(document.activeElement as HTMLElement);
    const move = (to: number) => {
      ev.preventDefault();
      list[(to + list.length) % list.length]?.focus();
    };
    switch (ev.key) {
      case 'ArrowDown':
        return move(i + 1);
      case 'ArrowUp':
        return move(i < 0 ? -1 : i - 1);
      case 'Home':
        return move(0);
      case 'End':
        return move(-1);
      case 'ArrowRight': {
        if (inSub) return;
        const el = document.activeElement as HTMLElement | null;
        const index = Number(el?.dataset.entry);
        const entry = entries[index];
        if (el && entry?.kind === 'item' && entry.submenu && !entry.disabled) {
          ev.preventDefault();
          openSub(index, el);
          setTimeout(() => items(subRef.current)[0]?.focus(), 0);
        }
        return;
      }
      case 'ArrowLeft':
      case 'Escape':
        ev.preventDefault();
        ev.stopPropagation();
        if (inSub) {
          const back = mainRef.current?.querySelector<HTMLElement>(`[data-entry="${sub?.index}"]`);
          setSub(null);
          back?.focus();
        } else if (ev.key === 'Escape') onClose();
        return;
      case 'Tab':
        ev.preventDefault();
        onClose();
    }
  };

  const subEntries = sub ? (entries[sub.index] as Extract<MenuEntry, { kind: 'item' }>).submenu : undefined;

  return createPortal(
    <div
      className="fixed inset-0 z-50"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
      onContextMenu={(e) => {
        e.preventDefault();
        onClose();
      }}
      onKeyDown={onKeyDown}
      data-testid="context-menu-overlay"
    >
      <div ref={mainRef} role="menu" aria-label={label} className={`${BOX} w-[240px]`} style={pos}>
        <Entries
          entries={entries}
          onHoverItem={(i, el) => {
            const e = entries[i];
            if (e?.kind === 'item' && e.submenu && !e.disabled) openSub(i, el);
            else setSub(null);
          }}
          onPick={pick}
        />
      </div>
      {sub && subEntries && (
        <div ref={subRef} role="menu" aria-label="Notifications" className={`${BOX} w-[260px]`} style={{ left: sub.left, top: sub.top }}>
          <Entries entries={subEntries} onHoverItem={() => {}} onPick={pick} />
        </div>
      )}
    </div>,
    document.body,
  );
}
