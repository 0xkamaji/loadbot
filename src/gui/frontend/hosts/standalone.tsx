import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { fixtureAdapter } from '../adapter/fixtures';
import { LoadbotMenu } from '../menu/LoadbotMenu';
import './host.css';

// The Tauri host owns native window controls. This entry also previews in a browser.
createRoot(document.getElementById('root')!).render(
  <StrictMode><LoadbotMenu adapter={fixtureAdapter} /></StrictMode>,
);
