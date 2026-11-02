import { FolderOpen, Globe, Plus } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { FormEvent, ReactNode } from 'react';
import { useClient } from '../lib/client';
import { hostBridge } from './hostBridge';
import type { RepoScan } from './hostBridge';
import { closeOverlay, getOverlay, useOverlay } from './overlay';
import { Field, Modal, btnCls, inputCls, labelCls, linkCls, primaryCls, rowHover } from './ui';
import { useSnapshot } from './useSnapshot';
import { joinPath, repoNameFromUrl, validateCloneUrl, validateNewPath, validatePath, validateProjectName } from './validation';

type View = 'home' | 'path' | 'clone' | 'create' | 'repos';

const errText = (e: unknown, fallback: string) => (e instanceof Error && e.message ? e.message : fallback);

function Choice({ icon, title, hint, onClick, border, first }: { icon: ReactNode; title: string; hint: string; onClick: () => void; border?: boolean; first?: boolean }) {
  return (
    <button type="button" data-autofocus={first ? '' : undefined} onClick={onClick} className={`flex w-full items-center gap-4 px-4 py-3 text-left ${rowHover} ${border ? 'border-t' : ''}`}>
      {icon}
      <span>
        <span className="block text-[14px] font-medium">{title}</span>
        <span className="block text-[12px] text-dim">{hint}</span>
      </span>
    </button>
  );
}

function Actions({ submit, busy, disabled }: { submit: string; busy: boolean; disabled?: boolean }) {
  return (
    <div className="flex gap-2">
      <button type="button" className={btnCls} onClick={closeOverlay}>Cancel</button>
      <button type="submit" className={primaryCls} disabled={busy || disabled}>{busy ? 'Working…' : submit}</button>
    </div>
  );
}

const Footer = ({ onBack, children }: { onBack: () => void; children: ReactNode }) => (
  <div className="mt-2 flex items-center justify-between">
    <button type="button" className={linkCls} onClick={onBack}>‹ Back</button>
    {children}
  </div>
);

/** Add a project: Browse, Clone from URL, Create new, multi-repo pick (mock `#addDlg`). */
export function AddProjectSheet() {
  const client = useClient();
  const overlay = useOverlay();
  const snap = useSnapshot();
  const projects = snap?.projects ?? [];
  const preset = overlay?.name === 'addProject' ? overlay.repos : undefined;

  const [view, setView] = useState<View>(preset ? 'repos' : 'home');
  const [path, setPath] = useState('~/Repos/');
  const [url, setUrl] = useState('');
  const [cloneDir, setCloneDir] = useState('~/Repos');
  const [name, setName] = useState('');
  const [newDir, setNewDir] = useState('~/Repos');
  const [gitInit, setGitInit] = useState(true);
  const [scan, setScan] = useState<{ root: string; repos: string[] } | null>(preset ?? null);
  const [off, setOff] = useState<Set<string>>(() => new Set()); // repos the user unticked
  const [busy, setBusy] = useState(false);
  const [showErrors, setShowErrors] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  // A view switch unmounts the control that had focus: move it to the new view's first field.
  const body = useRef<HTMLDivElement>(null);
  useEffect(() => {
    body.current?.querySelector<HTMLElement>('[data-autofocus]')?.focus();
  }, [view]);

  const go = (v: View) => {
    setView(v);
    setShowErrors(false);
    setFailure(null);
  };

  const run = async (job: () => Promise<void>, fallback: string) => {
    setBusy(true);
    setFailure(null);
    try {
      await job();
    } catch (e) {
      setFailure(errText(e, fallback));
    } finally {
      setBusy(false);
    }
  };

  const exists = (p: string) => projects.some((x) => x.path === p);
  const picked = scan ? scan.repos.filter((r) => !off.has(r) && !exists(r)) : [];

  const add = async (paths: string[]) => {
    for (const p of paths) await client.addProject({ path: p, name: null, color: null, icon: null });
    // The job can outlive the sheet (Esc during a clone): do not close whatever opened since.
    if (getOverlay()?.name === 'addProject') closeOverlay();
  };

  // A folder: add it, or, when it holds several repositories (and is none itself), offer each.
  const addFolder = async (p: string) => {
    const found: RepoScan = await hostBridge().scanRepos(p);
    if (!found.is_repo && found.repos.length > 1) {
      setScan({ root: p, repos: found.repos });
      setOff(new Set());
      go('repos');
      return;
    }
    await add([p]);
  };

  const browse = () => {
    const picker = hostBridge().pickFolder();
    if (!picker) return go('path');
    void run(async () => {
      const chosen = await picker;
      if (chosen) await addFolder(chosen);
    }, 'Could not open the folder');
  };

  const cloneName = repoNameFromUrl(url);
  const errors = {
    path: validateNewPath(path, projects),
    url: validateCloneUrl(url),
    cloneDir: validatePath(cloneDir, 'Clone into'),
    name: validateProjectName(name),
    newDir: validatePath(newDir, 'Location'),
  };
  const cloneTarget = !errors.url && !errors.cloneDir && cloneName ? joinPath(cloneDir, cloneName) : '';
  const createTarget = !errors.name && !errors.newDir ? joinPath(newDir, name) : '';
  const dup = (p: string) => (p && projects.some((x) => x.path === p) ? 'This folder is already a project' : null);
  const urlErr = errors.url ?? (cloneName ? validateProjectName(cloneName) : 'Could not read a repository name from the URL');
  const cloneErr = urlErr ?? errors.cloneDir ?? dup(cloneTarget);
  const createErr = errors.name ?? errors.newDir ?? dup(createTarget);

  const form = (err: string | null, job: () => Promise<void>, fallback: string) => (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    setShowErrors(true);
    if (!err) void run(job, fallback);
  };
  const shown = (e: string | null) => (showErrors ? e : null);

  const field = (id: string, label: string, value: string, set: (v: string) => void, err: string | null, ph = '', first = false) => (
    <>
      <label htmlFor={id} className={labelCls}>{label}</label>
      <input
        id={id}
        className={`${inputCls} mb-3`}
        value={value}
        placeholder={ph}
        spellCheck={false}
        data-autofocus={first ? '' : undefined}
        aria-invalid={!!shown(err)}
        aria-describedby={shown(err) ? `${id}-err` : undefined}
        onChange={(e) => set(e.target.value)}
      />
      <Field id={`${id}-err`} error={shown(err)} />
    </>
  );

  return (
    <Modal label="Add a project" onClose={closeOverlay} tone="bg-black/25" panelClass="relative mt-20 w-[520px] rounded-2xl p-6">
      <button type="button" onClick={closeOverlay} className="absolute right-4 top-4 grid h-[26px] w-7 place-items-center rounded-md text-dim hover:bg-sel" aria-label="Close">✕</button>
      <h3 className="mb-5 text-[20px] font-semibold">Add a project</h3>
      <div ref={body}>

      {view === 'home' && (
        <div>
          <div className="overflow-hidden rounded-xl bg-side ring-1 ring-line">
            <Choice first icon={<FolderOpen className="size-5 flex-none" strokeWidth={1.6} />} title="Browse folder" hint="Local project, Git repo, or folder with many repos" onClick={browse} />
          </div>
          <div className="mb-2 mt-5 text-[11px] font-semibold uppercase tracking-wide text-dim">Other ways to add</div>
          <div className="overflow-hidden rounded-xl bg-side ring-1 ring-line">
            <Choice icon={<Globe className="size-5 flex-none" strokeWidth={1.6} />} title="Clone from URL" hint="Clone a remote Git repository" onClick={() => go('clone')} />
            <Choice border icon={<Plus className="size-5 flex-none" strokeWidth={1.6} />} title="Create new project" hint="Start from an empty folder" onClick={() => go('create')} />
          </div>
          <div className="mt-4 text-[11px] text-dim">Tip: drag a folder from Finder onto the sidebar.</div>
          {failure && <div role="alert" className="mt-3 text-[12px] text-red">{failure}</div>}
        </div>
      )}

      {view === 'path' && (
        <form onSubmit={form(errors.path, () => addFolder(path.trim()), 'Could not add the folder')}>
          {field('add-path', 'Folder path', path, setPath, errors.path, '', true)}
          {failure && <div role="alert" className="mb-3 text-[12px] text-red">{failure}</div>}
          <Footer onBack={() => go('home')}><Actions submit="Add ⏎" busy={busy} /></Footer>
        </form>
      )}

      {view === 'clone' && (
        <form onSubmit={form(cloneErr, async () => { await hostBridge().cloneRepo(url.trim(), cloneTarget); await add([cloneTarget]); }, 'Clone failed')}>
          {field('clone-url', 'Repository URL', url, setUrl, urlErr, 'https://github.com/owner/repo.git or git@…', true)}
          {field('clone-dir', 'Clone into', cloneDir, setCloneDir, errors.cloneDir ?? (urlErr ? null : dup(cloneTarget)))}
          {failure && <div role="alert" className="mb-3 text-[12px] text-red">{failure}</div>}
          <Footer onBack={() => go('home')}><Actions submit="Clone ⏎" busy={busy} /></Footer>
        </form>
      )}

      {view === 'create' && (
        <form onSubmit={form(createErr, async () => { await hostBridge().createProject(createTarget, gitInit); await add([createTarget]); }, 'Could not create the project')}>
          {field('new-name', 'Project name', name, setName, errors.name, 'my-project', true)}
          {field('new-dir', 'Location', newDir, setNewDir, errors.newDir ?? (errors.name ? null : dup(createTarget)))}
          <label className="mb-3 flex items-center gap-2 text-[12px]">
            <input type="checkbox" checked={gitInit} onChange={(e) => setGitInit(e.target.checked)} />
            Initialize git repository <span className="text-dim">(optional)</span>
          </label>
          {failure && <div role="alert" className="mb-3 text-[12px] text-red">{failure}</div>}
          <Footer onBack={() => go('home')}><Actions submit="Create ⏎" busy={busy} /></Footer>
        </form>
      )}

      {view === 'repos' && scan && (
        <form onSubmit={(e) => { e.preventDefault(); if (!busy && picked.length) void run(() => add(picked),'Could not add the repositories'); }}>
          <p className="mb-2 text-[12px] text-dim" data-autofocus tabIndex={-1}>
            <span className="font-mono">{scan.root}</span> holds {scan.repos.length} repositories. Pick the ones to add.
          </p>
          <ul className="mb-3 max-h-[260px] space-y-0.5 overflow-y-auto font-mono text-[12px]" aria-label="Repositories">
            {scan.repos.map((r) => {
              const have = exists(r);
              return (
                <li key={r}>
                  <label className={`flex items-center gap-2 rounded-md px-2 py-1 ${rowHover}`}>
                    <input
                      type="checkbox"
                      checked={!have && !off.has(r)}
                      disabled={have}
                      onChange={(e) => setOff((s) => { const n = new Set(s); if (e.target.checked) n.delete(r); else n.add(r); return n; })}
                    />
                    <span className="truncate">{r}</span>
                    {have && <span className="ml-auto font-sans text-dim">already added</span>}
                  </label>
                </li>
              );
            })}
          </ul>
          {failure && <div role="alert" className="mb-3 text-[12px] text-red">{failure}</div>}
          <Footer onBack={() => go('home')}>
            <div className="flex gap-2">
              <button type="button" className={btnCls} onClick={closeOverlay}>Cancel</button>
              <button type="submit" className={primaryCls} disabled={busy || picked.length === 0}>
                {busy ? 'Working…' : `Add ${picked.length} ${picked.length === 1 ? 'project' : 'projects'} ⏎`}
              </button>
            </div>
          </Footer>
        </form>
      )}
      </div>
    </Modal>
  );
}
