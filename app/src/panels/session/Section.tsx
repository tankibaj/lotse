import type { ReactNode } from 'react';

/** The mock's `sec`: a titled block with a hairline above it (none on the first), the right-hand note dimmed. */
export function Section({ title, extra, testId, children }: { title: ReactNode; extra?: ReactNode; testId?: string; children: ReactNode }) {
  return (
    <section data-testid={testId} className="border-t border-line py-3 first:border-t-0 first:pt-0">
      <div className="mb-2 flex items-center text-[12px] font-medium">
        {title}
        {extra !== undefined && extra !== false && <span className="ml-auto font-normal text-dim">{extra}</span>}
      </div>
      {children}
    </section>
  );
}
