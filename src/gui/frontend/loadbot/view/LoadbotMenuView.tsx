import { useId } from 'react';
import { projectKey, selectionKey, shortcutKey } from '../identity';
import type { LoadbotActions, LoadbotState } from '../application/controller';
import { ApplicationFrame, BottomDrawer, Button, Icon, IconButton, MenuList, MenuRow, Panel, StatusDisplay, type ShellCallbacks } from '../../ui/components';
import mascot from '../../../loadbot-gui-assets/assets/branding/loadbot-header.png';
import { ShortcutDetails } from './ShortcutDetails';
import './menu.css';

/** Pure Loadbot layout: no adapter, fixture, native API, or asynchronous work. */
export function LoadbotMenuView({ state, actions, host }: {
  state: LoadbotState; actions: LoadbotActions; host?: ShellCallbacks;
}) {
  const drawerId = useId();
  const unavailableId = useId();
  const { inventory, project, shortcut, drawerOpen } = state;
  const projects = inventory.status === 'ready' ? inventory.projects : [];
  return <ApplicationFrame label="Loadbot menu">
    <header className="lb-header">
      <img className="lb-mascot" src={mascot} alt="" />
      <h1>LOADBOT</h1>
      <span className="lb-fixture-badge">FIXTURE PREVIEW<span>Phase 1 · no live operations</span></span>
      {host?.onClose && <IconButton icon="close" label="Close Loadbot menu" onClick={host.onClose} />}
    </header>
    <div className="lb-workspace">
      <Panel className="lb-sidebar" aria-label="Projects panel">
        <h2>PROJECTS</h2>
        <MenuList label="Projects">
          {projects.map((item) => <MenuRow key={projectKey(item)} icon="folder" selected={item === project}
            title={`${item.catalog}/${item.tool}`} onClick={() => actions.selectProject(projectKey(item))}>
            {item.tool}<small>{item.catalog}</small>
          </MenuRow>)}
          {inventory.status === 'loading' && <StatusDisplay>Loading fixture projects…</StatusDisplay>}
          {inventory.status === 'error' && <StatusDisplay>Fixture unavailable: {inventory.message ?? 'Could not read fixture data.'}</StatusDisplay>}
          {inventory.status === 'ready' && !projects.length && <StatusDisplay>No fixture projects available.</StatusDisplay>}
        </MenuList>
        <div className="lb-project-actions">
          <Button disabled aria-describedby={unavailableId}>+ Add project</Button>
          <Button disabled aria-describedby={unavailableId}>Refresh catalog</Button>
          <p className="lb-note" id={unavailableId}>Catalog and folder actions are not connected.</p>
        </div>
      </Panel>
      <Panel className="lb-shortcut-panel" aria-label="Shortcuts and details">
        <h2>SHORTCUTS <span>/ {project?.tool ?? 'Select a project'}</span></h2>
        <MenuList label="Shortcuts">
          {project?.entries.map((item) => <MenuRow key={shortcutKey(item)} icon="arrow-right" selected={item === shortcut}
            title={`${item.name} [${item.source === 'catalog' ? 'shared' : 'personal'}]`} onClick={() => actions.selectShortcut(shortcutKey(item))}>
            {item.name}{project.entries.some((other) => other !== item && other.name === item.name) && <small>[{item.source === 'catalog' ? 'shared' : 'personal'}]</small>}
          </MenuRow>)}
          {project && !project.entries.length && <StatusDisplay>No shortcuts in this fixture project.</StatusDisplay>}
        </MenuList>
        <ShortcutDetails key={project && shortcut ? selectionKey(project, shortcut) : 'empty'} state={state} actions={actions} />
      </Panel>
    </div>
    <BottomDrawer open={drawerOpen} id={drawerId} label="Terminal placeholder">
      <h2>TERMINAL / NOT CONNECTED</h2>
      <p>Layout placeholder for a future terminal.</p>
      <p>No shell session, command input, or output is connected.</p>
    </BottomDrawer>
    <footer className="lb-toolbar">
      <Button aria-expanded={drawerOpen} aria-controls={drawerId} aria-pressed={drawerOpen} onClick={actions.toggleDrawer}>
        <Icon name="terminal" inverse={drawerOpen} />Terminal
      </Button>
      <Button disabled aria-describedby={unavailableId}>Open project folder</Button>
    </footer>
  </ApplicationFrame>;
}
