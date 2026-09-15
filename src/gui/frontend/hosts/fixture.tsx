import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { fixtureMenuDependencies } from './fixtureComposition';
import { LoadbotMenu } from '../loadbot/LoadbotMenu';
import './host.css';

if (import.meta.env.DEV) {
  createRoot(document.getElementById('root')!).render(
    <StrictMode><LoadbotMenu {...fixtureMenuDependencies} /></StrictMode>,
  );
}
