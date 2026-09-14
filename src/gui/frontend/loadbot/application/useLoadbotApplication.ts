import { useEffect, useMemo, useSyncExternalStore } from 'react';
import type { LoadbotAdapter } from '../contract';
import { createLoadbotApplication } from './controller';
import { noSampleForms, type SampleForms } from './sampleForms';

/** React binding only. Async reads and deterministic transitions live in the controller. */
export function useLoadbotApplication(adapter: LoadbotAdapter, sampleForms: SampleForms = noSampleForms) {
  const application = useMemo(() => createLoadbotApplication(adapter, sampleForms), [adapter, sampleForms]);
  const state = useSyncExternalStore(application.subscribe, application.getSnapshot, application.getSnapshot);
  useEffect(() => application.start(), [application]);
  return { state, actions: application.actions };
}
