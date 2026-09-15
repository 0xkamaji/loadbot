import { invoke, isTauri } from '@tauri-apps/api/core';
import type { WorkspaceLayoutStore } from '../loadbot/view/workspaceLayout';

/** Native presentation persistence. The Rust host treats the document as opaque. */
export const tauriWorkspaceLayoutStore: WorkspaceLayoutStore = {
  async read() {
    if (!isTauri()) return undefined;
    const contents = await invoke<unknown>('read_loadbot_workspace_layout');
    return typeof contents === 'string' ? contents : undefined;
  },
  async write(contents) {
    if (!isTauri()) return;
    await invoke('write_loadbot_workspace_layout', { contents });
  },
};
