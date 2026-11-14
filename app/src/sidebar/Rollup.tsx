import type { RollupSummary } from '../lib/state/selectors';
import { StatusGlyph } from './StatusGlyph';

/** What a collapsed project or workspace row shows: need-you counts (▲2 ✕1), else the top state; idle shows nothing. */
export function Rollup({ sum }: { sum: RollupSummary }) {
  return (
    <span className="flex items-center gap-1.5">
      {sum.perm > 0 && (
        <span className="inline-flex items-center gap-0.5 text-[11px] text-orange tabular-nums" data-rollup="perm">
          <StatusGlyph status="perm" />
          {sum.perm > 1 ? sum.perm : ''}
        </span>
      )}
      {sum.fail > 0 && (
        <span className="inline-flex items-center gap-0.5 text-[11px] text-red tabular-nums" data-rollup="fail">
          <StatusGlyph status="fail" />
          {sum.fail > 1 ? sum.fail : ''}
        </span>
      )}
      {sum.perm === 0 && sum.fail === 0 && sum.top !== 'idle' && sum.top !== 'unknown' && (
        <span data-rollup={sum.top}>
          <StatusGlyph status={sum.top} />
        </span>
      )}
    </span>
  );
}
