import type { ReactNode } from 'react';
import { useOpenPanel } from '../lib/ui/panel';
import type { PanelName } from '../lib/ui/panel';
import { useSelectedSessionId } from '../lib/ui/selection';
import { ChangesPanel } from './changes';
import { FilesPanel } from './files';
import { SessionPanel } from './session';
import { TranscriptPanel } from './transcript';
import { UsagePanel } from './usage';

const TITLE: Record<PanelName, string> = {
  session: 'Session',
  changes: 'Changes',
  files: 'Files',
  transcript: 'Transcript',
  usage: 'Usage & time',
};

/** The mock's `#panel`: 340 px beside the panes, scrolls, one panel at a time. Renders nothing while closed. */
export function RightPanel() {
  const open = useOpenPanel();
  const sessionId = useSelectedSessionId();
  if (!open) return null;
  const body: Record<PanelName, ReactNode> = {
    session: <SessionPanel sessionId={sessionId} />,
    changes: <ChangesPanel sessionId={sessionId} />,
    files: <FilesPanel sessionId={sessionId} />,
    transcript: <TranscriptPanel sessionId={sessionId} />,
    usage: <UsagePanel sessionId={sessionId} />,
  };
  return (
    <aside className="w-[340px] shrink-0 overflow-y-auto border-l bg-side p-4" aria-label={TITLE[open]}>
      {body[open]}
    </aside>
  );
}
