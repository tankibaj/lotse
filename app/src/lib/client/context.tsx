import { createContext, useContext } from 'react';
import type { ReactNode } from 'react';
import type { DaemonClient } from './DaemonClient';

const ClientContext = createContext<DaemonClient | null>(null);

/** Provides the `DaemonClient` to the tree. The shell mounts it once; UI code only calls `useClient`. */
export function ClientProvider({ client, children }: { client: DaemonClient; children: ReactNode }) {
  return <ClientContext.Provider value={client}>{children}</ClientContext.Provider>;
}

export function useClient(): DaemonClient {
  const client = useContext(ClientContext);
  if (!client) throw new Error('useClient: no ClientProvider above this component');
  return client;
}
