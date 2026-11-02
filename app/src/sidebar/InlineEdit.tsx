import { useState } from 'react';

/** Rename in place: Enter or blur commits (when changed and not empty), Esc cancels. */
export function InlineEdit({
  value,
  label,
  onCommit,
  onCancel,
  className = '',
}: {
  value: string;
  label: string;
  onCommit: (next: string) => void;
  onCancel: () => void;
  className?: string;
}) {
  const [text, setText] = useState(value);
  const [done, setDone] = useState(false);
  const finish = (commit: boolean) => {
    if (done) return;
    setDone(true);
    const next = text.trim();
    if (commit && next !== '' && next !== value) onCommit(next);
    else onCancel();
  };
  return (
    <input
      className={`min-w-0 flex-1 rounded bg-win px-1 text-[13px] text-txt ring-1 ring-blue outline-none ${className}`}
      aria-label={label}
      value={text}
      autoFocus
      spellCheck={false}
      onFocus={(e) => e.currentTarget.select()}
      onChange={(e) => setText(e.target.value)}
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === 'Enter') finish(true);
        else if (e.key === 'Escape') finish(false);
      }}
      onBlur={() => finish(true)}
    />
  );
}
