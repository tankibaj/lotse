import { useEffect, useRef } from 'react';
import type { CSSProperties, KeyboardEvent as ReactKeyboardEvent, ReactNode, RefObject } from 'react';

// Shared chrome for the overlays, copied from the mock's classes.

export const inputCls = 'w-full font-mono text-[12px] rounded-md px-2 py-1.5 ring-1 ring-line bg-side text-txt';
export const labelCls = 'block text-[12px] mb-1 text-dim';
export const btnCls = 'px-3 py-1 rounded-md bg-black/5 dark:bg-white/10';
export const primaryCls = 'px-3 py-1 rounded-md text-white bg-blue disabled:opacity-50';
export const linkCls = 'text-[12px] text-blue';
export const rowHover = 'hover:bg-black/5 dark:hover:bg-white/5';

const FOCUSABLE = 'a[href],button:not([disabled]),input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])';

/**
 * Focus handling for a dialog: on mount move focus inside (`[data-autofocus]`, else the first focusable)
 * so keys stop reaching the terminal behind it; on unmount give focus back. Returns the `onKeyDown`
 * that keeps Tab inside.
 */
export function useDialogFocus(box: RefObject<HTMLElement | null>) {
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    const el = box.current;
    if (el && !el.contains(document.activeElement)) {
      (el.querySelector<HTMLElement>('[data-autofocus]') ?? el.querySelector<HTMLElement>(FOCUSABLE))?.focus();
    }
    return () => prev?.focus?.();
  }, [box]);

  return (e: ReactKeyboardEvent) => {
    if (e.key !== 'Tab' || !box.current) return;
    const items = [...box.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
    const first = items[0];
    const last = items[items.length - 1];
    if (!first || !last) return;
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  };
}

/**
 * Dimmed backdrop + dialog box. Backdrop click and Esc close; Tab stays inside; focus returns to the
 * previously focused element on close.
 */
export function Modal({
  label,
  onClose,
  tone = 'bg-black/20',
  panelClass,
  panelStyle,
  zClass = 'z-40',
  children,
}: {
  label: string;
  onClose: () => void;
  tone?: string;
  panelClass: string;
  panelStyle?: CSSProperties;
  zClass?: string;
  children: ReactNode;
}) {
  const box = useRef<HTMLDivElement>(null);
  const trap = useDialogFocus(box);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    };
    addEventListener('keydown', onKey);
    return () => removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div
      className={`fixed inset-0 overflow-y-auto ${zClass} ${tone}`}
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
      data-testid="overlay-backdrop"
    >
      <div
        ref={box}
        role="dialog"
        aria-modal="true"
        aria-label={label}
        onKeyDown={trap}
        style={panelStyle}
        className={`mx-auto max-w-[92vw] bg-win text-txt shadow-2xl ring-1 ring-black/10 ${panelClass}`}
      >
        {children}
      </div>
    </div>
  );
}

/** `⌘K`-style key cap used in lists. */
export const Kbd = ({ children }: { children: ReactNode }) => <kbd className="font-mono">{children}</kbd>;

export const Field = ({ error, id }: { error: string | null | undefined; id: string }) =>
  error ? (
    <div id={id} role="alert" className="-mt-2 mb-3 text-[11px] text-red">
      {error}
    </div>
  ) : null;
