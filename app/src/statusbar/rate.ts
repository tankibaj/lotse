import type { RateLimit } from '../lib/client/types';
import { clampPct } from '../sidebar/modData';

// Rate-limit meter of the footer (proto 5, from the Claude mod): one chip per window of the agent's plan.

export type RateLevel = 'ok' | 'warn' | 'crit';

export const rateLevel = (pct: number): RateLevel => (pct >= 90 ? 'crit' : pct >= 75 ? 'warn' : 'ok');

const WINDOW: Record<string, { short: string; long: string }> = {
  five_hour: { short: '5h', long: 'Five-hour' },
  seven_day: { short: '7d', long: 'Seven-day' },
};

export const windowLabel = (kind: string): string => WINDOW[kind]?.short ?? kind;

/** `resets in 2h`, `resets in 1h 05m`, `resets in 3d 2h`, `resets now`. */
export function resetsIn(resetsAt: number, now: number): string {
  const min = Math.round((resetsAt - now) / 60_000);
  if (min <= 0) return 'resets now';
  const h = Math.floor(min / 60);
  const m = min % 60;
  if (h >= 48) return `resets in ${Math.floor(h / 24)}d${h % 24 ? ` ${h % 24}h` : ''}`;
  if (h > 0) return `resets in ${h}h${m ? ` ${String(m).padStart(2, '0')}m` : ''}`;
  return `resets in ${m}m`;
}

export const rateTitle = (r: RateLimit, now: number): string => `${WINDOW[r.kind]?.long ?? r.kind} window: ${clampPct(r.pct)}% used · ${resetsIn(r.resets_at, now)}`;
