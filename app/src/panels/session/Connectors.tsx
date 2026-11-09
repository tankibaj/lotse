import { useState } from 'react';
import type { Connector, ConnectorStatus, SessionId } from '../../lib/client/types';
import { configOpener } from './configOpen';
import { Section } from './Section';

const SHOWN = 3;

const DOT: Record<ConnectorStatus['state'], string> = {
  configured: 'var(--gray)',
  connected: 'var(--green)',
  needs_auth: 'var(--orange)',
  failed: 'var(--red)',
  unknown: 'var(--gray)',
};

const STATE_LABEL: Record<ConnectorStatus['state'], string> = {
  configured: 'configured',
  connected: 'connected',
  needs_auth: 'needs auth',
  failed: 'failed',
  unknown: 'unknown',
};

const baseName = (path: string) => path.split('/').pop() || path;

function Status({ c, live, retry }: { c: Connector; live: boolean; retry: () => void }) {
  const s = c.status;
  if (s.state === 'connected') return <span className="text-[11px] tabular-nums text-dim">{s.tools} tools</span>;
  if (s.state === 'needs_auth') return <span className="text-[11px] text-orange">Sign in…</span>;
  if (s.state !== 'failed') return null;
  return (
    <span className="text-[11px] text-red" title={s.reason}>
      Failed
      {live && (
        <>
          {' · '}
          <button className="underline-offset-2 hover:underline" onClick={retry}>
            Retry
          </button>
        </>
      )}
    </span>
  );
}

/** "Manage": one config file opens directly; several (project and user config) pick one from a short list. */
function Manage({ sessionId, paths }: { sessionId: SessionId; paths: string[] }) {
  const [menu, setMenu] = useState(false);
  const open = configOpener();
  const run = (path: string) => {
    setMenu(false);
    void open?.(sessionId, path).catch(() => {});
  };
  return (
    <span className="relative ml-auto">
      <button
        className="text-[11px] text-blue disabled:text-dim"
        disabled={!open}
        title={open ? undefined : 'Available in the Lotse app'}
        aria-haspopup={paths.length > 1 ? 'menu' : undefined}
        aria-expanded={paths.length > 1 ? menu : undefined}
        onClick={() => (paths.length > 1 ? setMenu(!menu) : run(paths[0]!))}
      >
        Manage
      </button>
      {menu && (
        <ul role="menu" className="absolute right-0 top-full z-10 mt-1 min-w-40 rounded-md bg-win py-1 text-[12px] shadow-lg ring-1 ring-black/10">
          {paths.map((p) => (
            <li key={p} role="none">
              <button role="menuitem" className="block w-full truncate px-3 py-1 text-left hover:bg-sel" title={p} onClick={() => run(p)}>
                {baseName(p)}
              </button>
            </li>
          ))}
        </ul>
      )}
    </span>
  );
}

export function Connectors({ sessionId, connectors, live, retry }: { sessionId: SessionId; connectors: Connector[]; live: boolean; retry: () => void }) {
  const [more, setMore] = useState(false);
  const shown = more ? connectors : connectors.slice(0, SHOWN);
  const paths = [...new Set(connectors.map((c) => c.config_path))];
  return (
    <Section
      title={
        <>
          Connectors <span className="ml-1.5 text-[11px] font-normal text-dim">MCP</span>
        </>
      }
      extra={paths.length > 0 && <Manage sessionId={sessionId} paths={paths} />}
    >
      {connectors.length === 0 ? (
        <div className="text-[12px] text-dim">No MCP servers configured.</div>
      ) : (
        <div data-testid="session-connectors">
          {shown.map((c) => (
            <div
              key={`${c.config_path}:${c.name}`}
              className="flex min-w-0 items-center gap-2.5 py-1 text-[13px]"
              title={`${c.name} · ${STATE_LABEL[c.status.state]}${c.status.state === 'connected' ? ` · ${c.status.tools} tools` : ''}`}
            >
              <span aria-hidden className="grid h-5 w-5 shrink-0 place-items-center rounded-md bg-win text-[11px] text-dim ring-1 ring-line">
                {c.name[0]}
              </span>
              <span className="flex-1 truncate">{c.name}</span>
              <Status c={c} live={live} retry={retry} />
              <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ background: DOT[c.status.state] }} role="img" aria-label={STATE_LABEL[c.status.state]} />
            </div>
          ))}
          {connectors.length > SHOWN && (
            <button className="mt-1 text-[12px] text-dim" onClick={() => setMore(!more)}>
              {more ? 'Show less' : `Show ${connectors.length - SHOWN} more`}
            </button>
          )}
        </div>
      )}
    </Section>
  );
}
