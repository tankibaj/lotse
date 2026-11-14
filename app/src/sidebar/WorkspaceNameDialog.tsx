import { useEffect, useId, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

/**
 * Name prompt for "New Workspace…" and "Rename…" (Tauri webviews have no `window.prompt`). Enter or the
 * button commits; Esc or a click outside cancels. `onSubmit` rejects with the daemon's message (a taken name),
 * which stays visible under the field so the name can be fixed.
 */
export function WorkspaceNameDialog({ title, initial = '', submitLabel, onSubmit, onCancel }: {
  title: string;
  initial?: string;
  submitLabel: string;
  onSubmit: (name: string) => Promise<void>;
  onCancel: () => void;
}) {
  const id = useId();
  const [name, setName] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const from = document.activeElement;
    input.current?.focus();
    input.current?.select();
    return () => {
      if (from instanceof HTMLElement && from.isConnected) from.focus();
    };
  }, []);

  const submit = async () => {
    const next = name.trim();
    if (next === '' || busy) return;
    setBusy(true);
    try {
      await onSubmit(next);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not save the workspace');
      setBusy(false);
    }
  };

  return createPortal(
    <div
      className="fixed inset-0 z-50 grid place-items-start justify-center bg-black/25 pt-24"
      onMouseDown={(e) => e.target === e.currentTarget && onCancel()}
      onKeyDown={(e) => {
        if (e.key === 'Escape') {
          e.preventDefault();
          onCancel();
        }
      }}
    >
      <form
        role="dialog"
        aria-modal="true"
        aria-labelledby={`${id}-t`}
        className="w-[380px] max-w-[92vw] rounded-xl bg-win p-5 text-txt shadow-2xl ring-1 ring-black/10"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <h3 id={`${id}-t`} className="mb-3 font-semibold">
          {title}
        </h3>
        <input
          ref={input}
          aria-label="Workspace name"
          aria-invalid={error !== null}
          aria-describedby={error ? `${id}-e` : undefined}
          value={name}
          maxLength={100}
          spellCheck={false}
          onChange={(e) => {
            setName(e.target.value);
            setError(null);
          }}
          className="w-full rounded-md bg-side px-2 py-1.5 text-[13px] text-txt ring-1 ring-line outline-none focus:ring-blue"
        />
        {error && (
          <div id={`${id}-e`} role="alert" className="mt-1.5 text-[11px] text-red">
            {error}
          </div>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <button type="button" className="rounded-md bg-black/5 px-3 py-1 dark:bg-white/10" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" disabled={name.trim() === '' || busy} className="rounded-md bg-blue px-3 py-1 text-white disabled:opacity-50">
            {submitLabel}
          </button>
        </div>
      </form>
    </div>,
    document.body,
  );
}
