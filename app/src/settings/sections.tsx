import { useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import type { AgentKind, AppearanceSettings, EventPrefs, NotificationSettings } from '../lib/client/types';
import { integrationBridge } from './integration';
import type { IntegrationId, IntegrationState } from './integration';
import { setTheme, getThemePref } from '../theme';
import type { ThemePref } from '../theme';
import { AGENTS } from '../sheets/validation';
import { INTERFACE_SIZE, SYSTEM_FONT, TERMINAL_SIZE, cssFamily, interfaceStack, terminalStack } from './appearance';
import { useInstalledFonts } from './fonts';
import type { SettingsDraft } from './useSettings';

const dim = 'text-dim';
const h2 = 'mb-4 text-[20px] font-semibold';
const check = 'accent-blue';

/** Agents whose status hooks Lotse injects per pane; `where` is the config Lotse leaves alone. */
const INTEGRATIONS: Partial<Record<AgentKind, { id: IntegrationId; where: string }>> = {
  claude_code: { id: 'claude', where: 'Claude settings' },
  opencode: { id: 'opencode', where: 'OpenCode config' },
};

/** "Remove old global hooks", only while an earlier Lotse version's global install is still present (app only). */
function CleanupControl({ kind, label }: { kind: AgentKind; label: string }) {
  const meta = INTEGRATIONS[kind];
  const bridge = integrationBridge();
  const [state, setState] = useState<IntegrationState | 'busy' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const id = meta?.id;
  useEffect(() => {
    if (!bridge || !id) return;
    let alive = true;
    bridge.status(id).then((s) => alive && setState(s), () => alive && setState(null));
    return () => {
      alive = false;
    };
  }, [bridge, id]);
  if (!bridge || !meta || state === null || state === 'missing') return null;
  const remove = async () => {
    setState('busy');
    setError(null);
    try {
      await bridge.uninstall(meta.id);
      setState(await bridge.status(meta.id));
    } catch (e) {
      setError(typeof e === 'string' ? e : e instanceof Error ? e.message : 'failed');
      setState(await bridge.status(meta.id).catch(() => null));
    }
  };
  return (
    <>
      <button
        type="button"
        className="text-[12px] text-blue disabled:opacity-50"
        aria-label={`Remove old global hooks for ${label}`}
        disabled={state === 'busy'}
        onClick={() => void remove()}
      >
        Remove old global hooks
      </button>
      {error && <span role="alert" className="text-[11px] text-red">{error}</span>}
    </>
  );
}

export function AgentsSection({ draft }: { draft: SettingsDraft }) {
  const { settings, launch, update } = draft;
  if (!settings) return null;
  return (
    <section aria-labelledby="sec-agents">
      <h2 id="sec-agents" className={h2}>Agents</h2>
      <p className={`pb-2 text-[12px] ${dim}`}>
        Command used when a new session starts. Add flags, a wrapper or a full path. Runs in the project folder with your login shell environment.
      </p>
      {AGENTS.map(({ kind, label }) => {
        const info = launch.find((l) => l.agent === kind);
        const def = info?.default_command ?? '';
        const value = settings.launch_commands[kind];
        const set = (v: string, delay?: number) => update((s) => { s.launch_commands[kind] = v; }, delay);
        return (
          <div key={kind} className="border-b py-3 last:border-0">
            <div className="mb-1.5 flex items-center gap-2">
              <label htmlFor={`cmd-${kind}`} className="font-medium">{label}</label>
              {info && (info.found
                ? <span className="text-[11px] text-green">{[info.version, info.integration].filter(Boolean).join(' · ') || 'found'}</span>
                : <span className="text-[11px] text-orange">not found on PATH</span>)}
              {INTEGRATIONS[kind] && <CleanupControl kind={kind} label={label} />}
              <button type="button" className="ml-auto text-[12px] text-blue" aria-label={`Reset ${label} command`} onClick={() => set(def)}>Reset</button>
            </div>
            {INTEGRATIONS[kind] && (
              <p className={`pb-1.5 text-[11px] ${dim}`} data-testid={`live-status-${INTEGRATIONS[kind].id}`}>
                Live status: automatic in Lotse panes (nothing is written to your {INTEGRATIONS[kind].where})
              </p>
            )}
            <input
              id={`cmd-${kind}`}
              className="w-full rounded-md bg-side px-2 py-1.5 font-mono text-[12px] ring-1 ring-line"
              value={value}
              placeholder={def}
              spellCheck={false}
              aria-label={`${label} launch command`}
              onChange={(e) => set(e.target.value, 400)}
              onBlur={(e) => (e.target.value.trim() === '' ? set(def) : draft.flush())}
            />
          </div>
        );
      })}
    </section>
  );
}

type EventKey = 'perm' | 'fail' | 'done' | 'sub_agent_done';
const EVENTS: { key: EventKey; glyph: string; tone: string; label: string }[] = [
  { key: 'perm', glyph: '▲', tone: 'text-orange', label: 'Needs permission' },
  { key: 'fail', glyph: '✕', tone: 'text-red', label: 'Failed' },
  { key: 'done', glyph: '✓', tone: 'text-green', label: 'Done' },
  { key: 'sub_agent_done', glyph: '✓', tone: 'text-green', label: 'Sub-agent done' },
];

export function NotificationsSection({ draft }: { draft: SettingsDraft }) {
  const { settings, update } = draft;
  if (!settings) return null;
  const n: NotificationSettings = settings.notifications;
  const toggle = (key: EventKey, field: keyof EventPrefs, on: boolean) => update((s) => { s.notifications[key][field] = on; });
  return (
    <section aria-labelledby="sec-notifications">
      <h2 id="sec-notifications" className={h2}>Notifications</h2>
      <table className="w-full text-[13px]">
        <thead>
          <tr className={`text-[12px] ${dim}`}>
            <th className="py-2 text-left font-normal">Event</th>
            <th className="font-normal">Banner</th>
            <th className="font-normal">Sound</th>
            <th className="font-normal">Allow/Deny in banner</th>
          </tr>
        </thead>
        <tbody>
          {EVENTS.map((e) => (
            <tr key={e.key} className="border-t">
              <td className="py-2"><span className={e.tone} aria-hidden="true">{e.glyph}</span> {e.label}</td>
              <td className="text-center"><input type="checkbox" className={check} aria-label={`${e.label} banner`} checked={n[e.key].banner} onChange={(ev) => toggle(e.key, 'banner', ev.target.checked)} /></td>
              <td className="text-center"><input type="checkbox" className={check} aria-label={`${e.label} sound`} checked={n[e.key].sound} onChange={(ev) => toggle(e.key, 'sound', ev.target.checked)} /></td>
              <td className={`text-center ${e.key === 'perm' ? '' : dim}`}>
                {e.key === 'perm'
                  ? <input type="checkbox" className={check} aria-label="Allow/Deny in banner" checked={n.perm.allow_deny} onChange={(ev) => toggle('perm', 'allow_deny', ev.target.checked)} />
                  : '—'}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <label className="mt-4 flex items-center gap-2">
        <input type="checkbox" className={check} checked={n.only_when_unfocused} onChange={(e) => update((s) => { s.notifications.only_when_unfocused = e.target.checked; })} />
        Only when Lotse is not focused on that session
      </label>
      <p className={`mt-4 text-[12px] ${dim}`}>Per-project overrides: right-click a project › Notifications.</p>
    </section>
  );
}

const THEMES: { pref: ThemePref; label: string }[] = [
  { pref: 'light', label: 'Light' },
  { pref: 'dark', label: 'Dark' },
  { pref: 'auto', label: 'Auto' },
];

/** One family, drawn in itself where the platform's select menu allows it (WebKit may ignore it). */
const fontOption = (f: string) => <option key={f} value={f} style={{ fontFamily: cssFamily(f) }}>{f}</option>;

/** The stored font when it is not among `known`: it stays selectable instead of silently changing. */
const missingOption = (font: string, known: readonly string[]) =>
  known.includes(font) ? null : <option key={`missing-${font}`} value={font}>{font} (not installed)</option>;

function FontPicker(props: {
  title: string;
  font: string;
  size: number;
  range: { min: number; max: number };
  onFont(v: string): void;
  onSize(v: number): void;
  children: ReactNode;
}) {
  const { title, font, size, range } = props;
  return (
    <div>
      <label htmlFor={`font-${title}`} className={`mb-1.5 block text-[12px] ${dim}`}>{title}</label>
      <select id={`font-${title}`} className="w-full rounded-md bg-side px-2 py-1.5" value={font} onChange={(e) => props.onFont(e.target.value)}>
        {props.children}
      </select>
      <div className="mt-2 flex items-center gap-2 text-[12px]">
        Size
        <input type="range" className="flex-1" min={range.min} max={range.max} value={size} aria-label={`${title} size`} onChange={(e) => props.onSize(Number(e.target.value))} />
        <span className="w-5 tabular-nums">{size}</span>
      </div>
    </div>
  );
}

export function AppearanceSection({ draft }: { draft: SettingsDraft }) {
  const { settings, update } = draft;
  const [theme, setThemeState] = useState<ThemePref>(getThemePref);
  const installed = useInstalledFonts();
  const otherFonts = useMemo(() => {
    const mono = new Set(installed.monospace);
    return installed.families.filter((f) => !mono.has(f));
  }, [installed]);
  if (!settings) return null;
  const a: AppearanceSettings = settings.appearance;
  const set = (patch: Partial<AppearanceSettings>, delay?: number) => update((s) => { Object.assign(s.appearance, patch); }, delay);
  return (
    <section aria-labelledby="sec-appearance">
      <h2 id="sec-appearance" className={h2}>Appearance</h2>
      <div className="space-y-5">
        <div>
          <div className={`mb-1.5 text-[12px] ${dim}`} id="theme-label">Theme</div>
          <div className="inline-flex rounded-md bg-line p-0.5 text-[12px]" role="group" aria-labelledby="theme-label">
            {THEMES.map((t) => (
              <button
                key={t.pref}
                type="button"
                className="rounded px-3 py-1"
                aria-pressed={theme === t.pref}
                onClick={() => { setTheme(t.pref); setThemeState(t.pref); set({ theme: t.pref }); }}
              >
                {t.label}
              </button>
            ))}
          </div>
        </div>
        <div className="grid grid-cols-2 gap-4">
          <FontPicker title="Interface font" font={a.interface_font} size={a.interface_font_size} range={INTERFACE_SIZE}
            onFont={(v) => set({ interface_font: v })} onSize={(v) => set({ interface_font_size: v }, 300)}>
            <option value={SYSTEM_FONT}>{SYSTEM_FONT}</option>
            {missingOption(a.interface_font, [SYSTEM_FONT, ...installed.families])}
            {installed.families.map(fontOption)}
          </FontPicker>
          <FontPicker title="Terminal font" font={a.terminal_font} size={a.terminal_font_size} range={TERMINAL_SIZE}
            onFont={(v) => set({ terminal_font: v })} onSize={(v) => set({ terminal_font_size: v }, 300)}>
            {missingOption(a.terminal_font, installed.families)}
            <optgroup label="Monospace">{installed.monospace.map(fontOption)}</optgroup>
            {otherFonts.length > 0 && <optgroup label="Other fonts">{otherFonts.map(fontOption)}</optgroup>}
          </FontPicker>
        </div>
        <div
          className="rounded-md border bg-term p-3 text-[13px]"
          style={{ fontFamily: terminalStack(a.terminal_font), fontSize: a.terminal_font_size }}
          data-testid="font-preview"
          data-interface-font={interfaceStack(a.interface_font) ?? ''}
        >
          <span className="text-tdim">❯</span> claude <span className="text-tdim">— preview</span>
        </div>
      </div>
    </section>
  );
}

export function RemoteSection() {
  return (
    <section aria-labelledby="sec-remote">
      <h2 id="sec-remote" className={h2}>Remote</h2>
      <label className="flex items-center gap-2">
        <input type="checkbox" className={check} checked={false} disabled readOnly />
        Allow access from devices on my network <span className={dim}>(off)</span>
      </label>
      <p className={`mt-2 text-[12px] ${dim}`}>Not available in Alpha: Lotse opens no network listener. Remote access arrives in a later release.</p>
    </section>
  );
}

export function PrivacySection() {
  return (
    <section aria-labelledby="sec-privacy">
      <h2 id="sec-privacy" className={h2}>Privacy</h2>
      <p><b>Zero telemetry.</b> Nothing leaves this Mac. No account, no sign-in, no analytics, no crash upload.</p>
      <p className={`mt-2 text-[12px] ${dim}`}>Network use: none by default. Update checks are off until you turn them on.</p>
    </section>
  );
}

const KEYS: [string, string][] = [
  ['Command palette', '⌘K'],
  ['Next session that needs you', '⌘J'],
  ['New terminal', '⌘T'],
  ['New Claude Code session', '⌥⌘T'],
  ['Add a project', '⇧⌘O'],
  ['Settings', '⌘,'],
  ['Toggle sidebar', '⌃⌘S'],
  ['Changes · Files · Transcript · Usage · Session', '⇧⌘D · ⇧⌘F · ⇧⌘T · ⇧⌘U · ⇧⌘I'],
  ['All shortcuts', '⌘/'],
];

export function ShortcutsSection() {
  return (
    <section aria-labelledby="sec-shortcuts">
      <h2 id="sec-shortcuts" className={h2}>Shortcuts</h2>
      <div className="grid grid-cols-[1fr_auto] gap-y-1.5 text-[13px]">
        {KEYS.map(([what, keys]) => (
          <div key={what} className="contents">
            <span>{what}</span>
            <kbd className="font-mono">{keys}</kbd>
          </div>
        ))}
      </div>
    </section>
  );
}
