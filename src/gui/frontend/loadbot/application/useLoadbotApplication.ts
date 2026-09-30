import { useEffect, useMemo, useSyncExternalStore } from 'react';
import type { LoadbotAdapter } from '../contract';
import { createLoadbotApplication } from './controller';

/** React binding only. Async reads and deterministic transitions live in the controller. */
export function useLoadbotApplication(adapter: LoadbotAdapter) {
  const application = useMemo(() => createLoadbotApplication(adapter), [adapter]);
  const state = useSyncExternalStore(application.subscribe, application.getSnapshot, application.getSnapshot);
  useEffect(() => application.start(), [application]);
  return { state, actions: application.actions };
}
