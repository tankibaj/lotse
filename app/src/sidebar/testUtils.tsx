import { render } from '@testing-library/react';
import type { ReactNode } from 'react';
import { ClientProvider, MockClient } from '../lib/client';
import { resetUiState } from '../lib/state';
import { selectSession } from '../lib/ui/selection';

/** Tests only: a static MockClient (no script) wrapped in the provider, with shared UI state reset. */
export function mount(ui: ReactNode, client = new MockClient({ script: false })) {
  selectSession(null);
  resetUiState();
  const view = render(<ClientProvider client={client}>{ui}</ClientProvider>);
  return { client, ...view };
}
