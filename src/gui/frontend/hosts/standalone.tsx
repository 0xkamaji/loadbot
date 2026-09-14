import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { fixtureMenuDependencies } from './fixtureComposition';
import { LoadbotMenu } from '../loadbot/LoadbotMenu';
import './host.css';

// The Tauri host owns native window controls. This entry also previews in a browser.
createRoot(document.getElementById('root')!).render(
  <StrictMode><LoadbotMenu {...fixtureMenuDependencies} /></StrictMode>,
);
