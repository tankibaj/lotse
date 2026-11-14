import { useEffect, useId, useRef } from 'react';
import { createPortal } from 'react-dom';

/** Small in-app confirmation (Tauri webviews do not give us `window.confirm`). Cancel has focus; Esc cancels. */
export function ConfirmDialog({ title, body, confirmLabel, onConfirm, onCancel }: {
  title: string;
  body: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const id = useId();
  const cancel = useRef<HTMLButtonElement>(null);
  const confirm = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const from = document.activeElement;
    cancel.current?.focus();
    return () => {
      if (from instanceof HTMLElement && from.isConnected) from.focus();
    };
  }, []);
  return createPortal(
    <div
      className="fixed inset-0 z-50 grid place-items-start justify-center bg-black/25 pt-24"
      onMouseDown={(e) => e.target === e.currentTarget && onCancel()}
      onKeyDown={(e) => {
        if (e.key === 'Escape') {
          e.preventDefault();
          onCancel();
        } else if (e.key === 'Tab') {
          e.preventDefault();
          (document.activeElement === cancel.current ? confirm : cancel).current?.focus();
        }
      }}
    >
      <div
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={`${id}-t`}
        aria-describedby={`${id}-b`}
        className="w-[380px] max-w-[92vw] rounded-xl bg-win p-5 text-txt shadow-2xl ring-1 ring-black/10"
      >
        <h3 id={`${id}-t`} className="mb-1 font-semibold">
          {title}
        </h3>
        <p id={`${id}-b`} className="text-[12px] text-dim">
          {body}
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button ref={cancel} className="rounded-md bg-black/5 px-3 py-1 dark:bg-white/10" onClick={onCancel}>
            Cancel
          </button>
          <button ref={confirm} className="rounded-md bg-[#D70015] px-3 py-1 text-white" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
