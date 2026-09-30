import type { LoadbotAdapter } from './contract';
import { useLoadbotApplication } from './application/useLoadbotApplication';
import type { ShellCallbacks } from '../ui/components';
import { LoadbotMenuView } from './view/LoadbotMenuView';
import { browserWorkspaceLayoutStore, type WorkspaceLayoutStore } from './view/workspaceLayout';

/** Public composition seam. A parent supplies capabilities and host integration. */
export function LoadbotMenu({ adapter, host, mode = 'local', workspaceLayoutStore = browserWorkspaceLayoutStore }: {
  adapter: LoadbotAdapter;
  host?: ShellCallbacks;
  mode?: 'local' | 'fixture';
  workspaceLayoutStore?: WorkspaceLayoutStore;
}) {
  const application = useLoadbotApplication(adapter);
  return <LoadbotMenuView {...application} host={host} mode={mode} workspaceLayoutStore={workspaceLayoutStore} />;
}
