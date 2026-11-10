import { useEffect, useRef, useState } from 'react';
import { useClient } from '../lib/client';
import type { AppearanceSettings } from '../lib/client/types';
import { getThemePref, setTheme } from '../theme';
import { emitUiCommand, onUiCommand } from '../lib/ui/commands';
import { inTauri } from '../sheets/hostBridge';
import { closeSettings, openSettings, useOverlay } from '../sheets/overlay';
import type { SettingsSection } from '../sheets/overlay';
import { useDialogFocus } from '../sheets/ui';
import { onMenuSettings } from './appMenu';
import { applyAppearance } from './appearance';
import { resetInstalledFonts } from './fonts';
import { ChildSessionsSetting } from './ChildSessionsSetting';
import { AgentsSection, AppearanceSection, NotificationsSection, PrivacySection, RemoteSection, ShortcutsSection } from './sections';
import { useSettings } from './useSettings';

const SECTIONS: SettingsSection[] = ['Agents', 'Notifications', 'Appearance', 'Remote', 'Privacy', 'Shortcuts'];

function SettingsView({ initial }: { initial: SettingsSection }) {
  const [section, setSection] = useState<SettingsSection>(initial);
  const draft = useSettings();
  const box = useRef<HTMLDivElement>(null);
  const trap = useDialogFocus(box);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') closeSettings();
    };
    addEventListener('keydown', onKey);
    return () => removeEventListener('keydown', onKey);
  }, []);

  // The installed fonts are asked once per Settings open (the Appearance pickers load them on first
  // use); closing forgets them, so fonts installed meanwhile show up next time.
  useEffect(() => resetInstalledFonts, []);

  return (
    <div ref={box} onKeyDown={trap} className="absolute inset-0 z-30 flex bg-win text-txt" role="dialog" aria-modal="true" aria-label="Settings" data-testid="settings-page">
      <aside className="flex w-[220px] shrink-0 flex-col border-r bg-side">
        <div className="flex h-11 items-center gap-2 px-3">
          {inTauri() ? (
            <span className="w-[52px] shrink-0" aria-hidden="true" />
          ) : (
            <>
              <span className="size-3 rounded-full bg-[#FF5F57]" />
              <span className="size-3 rounded-full bg-[#FEBC2E]" />
              <span className="size-3 rounded-full bg-[#28C840]" />
            </>
          )}
        </div>
        <button type="button" onClick={closeSettings} className="mx-2 mb-2 rounded-md px-3 py-1.5 text-left text-[13px] text-blue hover:bg-black/5 dark:hover:bg-white/5">
          ‹ Back to sessions <span className="text-[11px] text-dim">esc</span>
        </button>
        <nav className="space-y-0.5 px-2 text-[13px]" aria-label="Settings sections">
          {SECTIONS.map((s) => (
            // `aria-current`, not `aria-pressed`: a section is where you are, not a toggle. The highlight is a plain class
            // of the current render (no attribute selector, no transition), so it cannot linger on the section left behind.
            <button
              key={s}
              type="button"
              className={`w-full rounded-md px-3 py-1.5 text-left !transition-none ${s === section ? 'bg-sel text-blue' : 'hover:bg-black/5 dark:hover:bg-white/5'}`}
              aria-current={s === section ? 'page' : undefined}
              onClick={() => setSection(s)}
            >
              {s}
            </button>
          ))}
        </nav>
      </aside>
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[680px] px-8 py-8">
          {draft.error && <div role="alert" className="mb-4 text-[12px] text-red">{draft.error}</div>}
          {section === 'Agents' && <AgentsSection draft={draft} />}
          {section === 'Agents' && <ChildSessionsSetting draft={draft} />}
          {section === 'Notifications' && <NotificationsSection draft={draft} />}
          {section === 'Appearance' && <AppearanceSection draft={draft} />}
          {section === 'Remote' && <RemoteSection />}
          {section === 'Privacy' && <PrivacySection />}
          {section === 'Shortcuts' && <ShortcutsSection />}
        </div>
      </div>
    </div>
  );
}

/** Full-page Settings (mock `#set`). Listens for `openSettings` and ⌘,; applies the saved fonts at start. */
export function SettingsPage() {
  const client = useClient();
  const overlay = useOverlay();

  useEffect(() => {
    const off = onUiCommand('openSettings', () => openSettings());
    // ⌘, has one path per environment so it cannot fire twice: in the app it is the accelerator of the
    // menu's "Settings…" item (the event below); in the browser, which has no menu, this key handler.
    const offMenu = onMenuSettings(() => emitUiCommand('openSettings'));
    const onKey = (e: KeyboardEvent) => {
      if (inTauri()) return;
      if (e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey && e.code === 'Comma') {
        e.preventDefault();
        openSettings();
      }
    };
    addEventListener('keydown', onKey);
    return () => {
      off();
      offMenu();
      removeEventListener('keydown', onKey);
    };
  }, []);

  // Fonts and theme are saved in the daemon's settings: apply them at start and whenever they change.
  // The daemon wins over the local `lotse-theme` pref (which only serves until it has answered).
  useEffect(() => {
    let alive = true;
    const adopt = (a: AppearanceSettings) => {
      applyAppearance(a);
      if (a.theme !== getThemePref()) setTheme(a.theme);
    };
    client.getSettings().then((s) => alive && adopt(s.appearance)).catch(() => {});
    const off = client.onEvent((e) => e.event === 'settings_changed' && adopt(e.data.appearance));
    return () => {
      alive = false;
      off();
    };
  }, [client]);

  if (overlay?.name !== 'settings') return null;
  return <SettingsView key={overlay.section} initial={overlay.section} />;
}
