import type { ReactNode } from 'react';
import type { Status } from '../lib/client/types';
import { STATUS_LABEL } from '../lib/state/selectors';

// One icon set (spec/research.html#sidebar): shape plus colour so it reads without colour.
// Spinning ring = in progress, filled triangle = needs permission, filled ✕ circle = failed,
// filled ✓ circle = done, outline circle = idle (dashed: unknown). 14 px, 12 px for sub-agents.

const SHAPES: Record<Status, ReactNode> = {
  run: (
    <>
      <circle cx="8" cy="8" r="6" fill="none" stroke="var(--line)" strokeWidth="2" />
      <path d="M8 2a6 6 0 0 1 6 6" fill="none" stroke="var(--blue)" strokeWidth="2" strokeLinecap="round" />
    </>
  ),
  perm: (
    <>
      <path d="M8 1.8 14.8 13.8H1.2z" fill="var(--orange)" stroke="var(--orange)" strokeWidth="1.4" strokeLinejoin="round" />
      <path d="M8 6v3.4" stroke="#fff" strokeWidth="1.6" strokeLinecap="round" />
      <circle cx="8" cy="11.6" r=".9" fill="#fff" />
    </>
  ),
  fail: (
    <>
      <circle cx="8" cy="8" r="7" fill="var(--red)" />
      <path d="M5.7 5.7l4.6 4.6M10.3 5.7l-4.6 4.6" stroke="#fff" strokeWidth="1.6" strokeLinecap="round" />
    </>
  ),
  done: (
    <>
      <circle cx="8" cy="8" r="7" fill="var(--green)" />
      <path d="M4.9 8.2l2.1 2.1 4.2-4.4" fill="none" stroke="#fff" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
    </>
  ),
  idle: <circle cx="8" cy="8" r="6" fill="none" stroke="var(--gray)" strokeWidth="1.5" />,
  // Detecting: dashed ring around a dot, static (a spinner would read as work in progress).
  unknown: (
    <>
      <circle cx="8" cy="8" r="6" fill="none" stroke="var(--gray)" strokeWidth="1.5" strokeDasharray="2.5 2.5" />
      <circle cx="8" cy="8" r="1.5" fill="var(--gray)" />
    </>
  ),
};

export function StatusGlyph({ status, size = 14, label = STATUS_LABEL[status] }: { status: Status; size?: 14 | 12; label?: string }) {
  return (
    <svg
      className={`inline-block flex-none align-[-2px] ${status === 'run' ? 'spin' : ''}`}
      style={{ width: size, height: size, transformOrigin: '50% 50%' }}
      viewBox="0 0 16 16"
      role="img"
      aria-label={label}
      data-status={status}
    >
      <title>{label}</title>
      {SHAPES[status]}
    </svg>
  );
}
