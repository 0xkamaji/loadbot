import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { realMenuDependencies } from './realComposition';
import { LoadbotMenu } from '../loadbot/LoadbotMenu';
import './host.css';

// The native host owns window controls and the local inventory bridge on both platforms.
createRoot(document.getElementById('root')!).render(
  <StrictMode><LoadbotMenu {...realMenuDependencies} /></StrictMode>,
);
