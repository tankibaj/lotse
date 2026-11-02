export type ThemePref = 'light' | 'dark' | 'auto';

/** Same storage key as spec/mock.html. */
export const THEME_KEY = 'lotse-theme';

const isPref = (v: unknown): v is ThemePref => v === 'light' || v === 'dark' || v === 'auto';

export function getThemePref(): ThemePref {
  try {
    const v = localStorage.getItem(THEME_KEY);
    return isPref(v) ? v : 'auto';
  } catch {
    return 'auto';
  }
}

/** Resolve a preference to dark or not, given the OS setting. */
export const resolveDark = (pref: ThemePref, osDark: boolean): boolean => pref === 'dark' || (pref === 'auto' && osDark);

/** Apply a preference to <html> (the `dark` class) without saving it. */
export function applyTheme(pref: ThemePref): void {
  const osDark = typeof matchMedia === 'function' && matchMedia('(prefers-color-scheme: dark)').matches;
  document.documentElement.classList.toggle('dark', resolveDark(pref, osDark));
}

/** Save and apply a preference. */
export function setTheme(pref: ThemePref): void {
  try {
    localStorage.setItem(THEME_KEY, pref);
  } catch {
    // storage unavailable: still apply for this run
  }
  applyTheme(pref);
}

/** Apply the saved preference and follow OS changes while it is `auto`. Returns the cleanup. */
export function initTheme(): () => void {
  applyTheme(getThemePref());
  if (typeof matchMedia !== 'function') return () => {};
  const mq = matchMedia('(prefers-color-scheme: dark)');
  const onChange = () => {
    if (getThemePref() === 'auto') applyTheme('auto');
  };
  mq.addEventListener('change', onChange);
  return () => mq.removeEventListener('change', onChange);
}
