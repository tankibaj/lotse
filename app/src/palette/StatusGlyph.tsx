import type { Status } from '../lib/client/types';

const LABEL: Record<Status, string> = {
  run: 'In progress',
  perm: 'Needs permission',
  fail: 'Failed',
  done: 'Done',
  idle: 'Idle',
  unknown: 'Unknown',
};

/** The mock's status glyphs (`G`): spinner, warning triangle, red cross, green check, idle ring. */
export function StatusGlyph({ status }: { status: Status }) {
  const label = LABEL[status];
  return (
    <svg className={`inline-block size-3.5 flex-none ${status === 'run' ? 'spin' : ''}`} viewBox="0 0 16 16" role="img" aria-label={label}>
      <title>{label}</title>
      {status === 'run' && (
        <>
          <circle cx="8" cy="8" r="6" fill="none" stroke="var(--line)" strokeWidth="2" />
          <path d="M8 2a6 6 0 0 1 6 6" fill="none" stroke="var(--blue)" strokeWidth="2" strokeLinecap="round" />
        </>
      )}
      {status === 'perm' && (
        <>
          <path d="M8 1.8 14.8 13.8H1.2z" fill="var(--orange)" stroke="var(--orange)" strokeWidth="1.4" strokeLinejoin="round" />
          <path d="M8 6v3.4" stroke="#fff" strokeWidth="1.6" strokeLinecap="round" />
          <circle cx="8" cy="11.6" r=".9" fill="#fff" />
        </>
      )}
      {status === 'fail' && (
        <>
          <circle cx="8" cy="8" r="7" fill="var(--red)" />
          <path d="M5.7 5.7l4.6 4.6M10.3 5.7l-4.6 4.6" stroke="#fff" strokeWidth="1.6" strokeLinecap="round" />
        </>
      )}
      {status === 'done' && (
        <>
          <circle cx="8" cy="8" r="7" fill="var(--green)" />
          <path d="M4.9 8.2l2.1 2.1 4.2-4.4" fill="none" stroke="#fff" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
        </>
      )}
      {(status === 'idle' || status === 'unknown') && (
        <circle cx="8" cy="8" r="6" fill="none" stroke="var(--gray)" strokeWidth="1.5" strokeDasharray={status === 'unknown' ? '2 2' : undefined} />
      )}
    </svg>
  );
}
