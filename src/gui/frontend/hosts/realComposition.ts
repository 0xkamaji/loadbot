import { createTauriLoadbotAdapter } from './tauriInventoryAdapter';

// Native Windows/Linux both use this composition. No fixtures or sample forms.
export const realMenuDependencies = { adapter: createTauriLoadbotAdapter(), mode: 'local' as const };
