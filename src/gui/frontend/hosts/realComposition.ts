import { createTauriLoadbotAdapter } from './tauriInventoryAdapter';
import { tauriWorkspaceLayoutStore } from './tauriWorkspaceLayoutStore';

// Native Windows/Linux both use this composition. No fixtures or sample forms.
export const realMenuDependencies = {
  adapter: createTauriLoadbotAdapter(),
  workspaceLayoutStore: tauriWorkspaceLayoutStore,
  mode: 'local' as const,
};
