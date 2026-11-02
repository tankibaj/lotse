import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './index.css';
import { ClientProvider, createClient } from './lib/client';
import { Shell } from './shell';
import { initTheme } from './theme';

initTheme();
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
