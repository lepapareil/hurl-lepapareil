import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
import './styles.css';

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import hurlLogo from '../../../../art/logo-icon.svg';
import { App } from './App';
import { useStore } from './store';

// Browser mode: the icon of the page is the logo of Hurl, as everywhere else.
const icon = document.createElement('link');
icon.rel = 'icon';
icon.type = 'image/svg+xml';
icon.href = hurlLogo;
document.head.append(icon);

// Started once, outside of React (StrictMode mounts effects twice in dev).
void useStore.getState().init();

// The webview context menu (reload, inspect...) is replaced by the app menus.
window.addEventListener('contextmenu', (e) => {
  const target = e.target as HTMLElement;
  if (!target.closest('input, textarea, .cm-content, pre, code')) e.preventDefault();
});

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
