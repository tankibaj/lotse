import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './index.css';
import { ClientProvider, createClient } from './lib/client';
import { installContextMenuGuard } from './lib/ui/contextMenuGuard';
import { installNotificationOpen } from './lib/ui/notificationOpen';
import { Shell } from './shell';
import { initTheme } from './theme';

initTheme();
installContextMenuGuard();
installNotificationOpen();
const client = createClient();

const root = document.getElementById('root');
if (!root) throw new Error('#root missing');

createRoot(root).render(
  <StrictMode>
    <ClientProvider client={client}>
      <Shell />
    </ClientProvider>
  </StrictMode>,
);
