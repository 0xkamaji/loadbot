import { useEffect, useId, useRef, useState } from 'react';
import { projectKey, selectionKey, shortcutKey } from '../identity';
import type { LoadbotActions, LoadbotState } from '../application/controller';
import { ApplicationFrame, BottomDrawer, Button, Icon, IconButton, MenuList, MenuRow, Panel, StatusDisplay, type ShellCallbacks } from '../../ui/components';
import { clampSplit, Splitter } from '../../ui/Splitter';
import mascot from '../../../loadbot-gui-assets/assets/branding/loadbot-header.png';
import { ShortcutDetails } from './ShortcutDetails';
import { defaultPaneSizes, persistPaneSizes, restorePaneSizes, type PaneSizes } from './workspaceLayout';
import './menu.css';

/** Pure Loadbot layout: no adapter, fixture, native API, or asynchronous work. */
export function LoadbotMenuView({ state, actions, host, mode }: {
  state: LoadbotState; actions: LoadbotActions; host?: ShellCallbacks; mode: 'local' | 'fixture';
}) {
  const drawerId = useId();
  const { inventory, project, shortcut, drawerOpen } = state;
  const projects = inventory.status === 'ready' ? inventory.projects : [];
  const [paneSizes, setPaneSizes] = useState(restorePaneSizes);
  const workspaceRef = useRef<HTMLDivElement>(null);
  const rightRef = useRef<HTMLElement>(null);
  const mainTerminalRef = useRef<HTMLDivElement>(null);
  const projectLimits = () => {
    const total = workspaceRef.current?.clientWidth || 800;
    const min = Math.min(180, Math.max(120, total - 260));
    return { min, max: Math.max(min, Math.min(560, total - 268)) };
  };
  const shortcutLimits = () => {
    const total = rightRef.current?.clientHeight || 450;
    return { min: 110, max: Math.max(110, Math.min(640, total - 158)) };
  };
  const terminalLimits = () => {
    const total = mainTerminalRef.current?.clientHeight || 580;
    return { min: 88, max: Math.max(88, Math.min(440, total - 258)) };
  };
  const updatePane = (name: keyof PaneSizes, value: number) => setPaneSizes((current) => ({ ...current, [name]: value }));
  useEffect(() => persistPaneSizes(paneSizes), [paneSizes]);
  useEffect(() => {
    const clampToWindow = () => setPaneSizes((current) => {
      const next = {
        projects: clampSplit(current.projects, projectLimits()),
        shortcuts: clampSplit(current.shortcuts, shortcutLimits()),
        terminal: clampSplit(current.terminal, terminalLimits()),
      };
      return next.projects === current.projects && next.shortcuts === current.shortcuts && next.terminal === current.terminal
        ? current : next;
    });
    clampToWindow();
    window.addEventListener('resize', clampToWindow);
    const observer = typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(clampToWindow);
    for (const element of [workspaceRef.current, rightRef.current, mainTerminalRef.current]) {
      if (element) observer?.observe(element);
    }
    return () => { window.removeEventListener('resize', clampToWindow); observer?.disconnect(); };
  }, []);
  const catalog = project?.catalog ?? 'NO CATALOG';
  return <ApplicationFrame label="Loadbot menu">
    <header className="lb-header">
      <img className="lb-mascot" src={mascot} alt="" />
      <h1>LOADBOT</h1>
      <span className="lb-fixture-badge">{mode === 'fixture' ? 'FIXTURE PREVIEW' : 'LOCAL INVENTORY'}<span>{mode === 'fixture' ? 'Isolated test data' : 'Read-only workspace'}</span></span>
      <Button className="lb-catalog-context" disabled title="Catalog context; catalog management is not available in this phase" aria-label={`Catalog context: ${catalog}`}>
        <span>{catalog}</span><Icon name="chevron-down" />
      </Button>
      <Button className="lb-reload" disabled={inventory.status === 'loading'} onClick={actions.reloadInventory} title="Reread local Loadbot inventory">
        {inventory.status === 'loading' ? 'READING…' : 'RELOAD LOCAL'}
      </Button>
      {host?.onClose && <IconButton icon="close" label="Close Loadbot menu" onClick={host.onClose} />}
    </header>
    <div className={`lb-main-terminal ${drawerOpen ? '' : 'lb-terminal-closed'}`} ref={mainTerminalRef}
      style={drawerOpen ? { gridTemplateRows: `minmax(0, 1fr) 8px ${paneSizes.terminal}px` } : undefined}>
      <div className="lb-workspace" ref={workspaceRef} style={{ gridTemplateColumns: `${paneSizes.projects}px 8px minmax(0, 1fr)` }}>
        <Panel className="lb-sidebar" aria-label="Projects panel">
          <h2>PROJECTS</h2>
          <MenuList label="Projects">
            {projects.map((item) => {
              const id = projectKey(item);
              return <div className="lb-project-row" data-selected={item === project} key={id}>
                <MenuRow className="lb-project-select" icon="arrow-right" selected={item === project}
                  title={`${item.catalog}/${item.tool}`} onClick={() => actions.selectProject(id)}>
                  {item.tool}<small>{item.catalog}</small>
                </MenuRow>
                <IconButton className="lb-open-project" icon="folder" label={`Open project folder: ${item.tool} (${item.catalog})`}
                  title="Open project folder" disabled={state.projectFolder.status === 'opening' && state.projectFolder.projectId === id}
                  onClick={() => actions.openProjectFolder(id)} />
              </div>;
            })}
            {inventory.status === 'loading' && <StatusDisplay>{mode === 'fixture' ? 'Loading fixture projects…' : 'Reading local Loadbot inventory…'}</StatusDisplay>}
            {inventory.status === 'error' && <StatusDisplay>{mode === 'fixture' ? 'Fixture unavailable: ' : 'Inventory read failed: '}{inventory.message ?? (mode === 'fixture' ? 'Could not read fixture data.' : 'Could not read local Loadbot data.')}</StatusDisplay>}
            {inventory.status === 'ready' && !projects.length && <StatusDisplay>{mode === 'fixture' ? 'No fixture projects available.' : 'No projects with commands or shortcuts in local Loadbot data.'}</StatusDisplay>}
          </MenuList>
          {state.projectFolder.status !== 'idle' && <StatusDisplay>{state.projectFolder.status === 'opening'
            ? 'Opening project folder…' : state.projectFolder.message}</StatusDisplay>}
        </Panel>
        <Splitter orientation="vertical" label="Resize projects pane" value={paneSizes.projects} limits={projectLimits}
          onChange={(value) => updatePane('projects', value)} onCommit={(value) => updatePane('projects', value)}
          onReset={() => updatePane('projects', clampSplit(defaultPaneSizes.projects, projectLimits()))} />
        <Panel className="lb-shortcut-panel" aria-label="Shortcut workspace" ref={rightRef}
          style={{ gridTemplateRows: `${paneSizes.shortcuts}px 8px minmax(0, 1fr)` }}>
          <section className="lb-shortcut-list">
            <h2>SHORTCUTS <span>/ {project?.tool ?? 'Select a project'}</span></h2>
            <MenuList label="Shortcuts">
              {project?.entries.map((item) => <MenuRow key={shortcutKey(item)} icon="arrow-right" selected={item === shortcut}
                title={`${item.name} [${item.source === 'catalog' ? 'shared' : 'personal'}]`} onClick={() => actions.selectShortcut(shortcutKey(item))}>
                {item.name}{project.entries.some((other) => other !== item && other.name === item.name) && <small>[{item.source === 'catalog' ? 'shared' : 'personal'}]</small>}
              </MenuRow>)}
              {!project && inventory.status === 'ready' && projects.length > 0 && <StatusDisplay>Select a project to inspect its shortcuts.</StatusDisplay>}
              {project && !project.entries.length && <StatusDisplay>{mode === 'fixture' ? 'No shortcuts in this fixture project.' : 'No shortcuts in this project.'}</StatusDisplay>}
            </MenuList>
          </section>
          <Splitter orientation="horizontal" label="Resize shortcuts and selected shortcut" value={paneSizes.shortcuts} limits={shortcutLimits}
            onChange={(value) => updatePane('shortcuts', value)} onCommit={(value) => updatePane('shortcuts', value)}
            onReset={() => updatePane('shortcuts', clampSplit(defaultPaneSizes.shortcuts, shortcutLimits()))} />
          <section className="lb-selected-shortcut" aria-label="Selected shortcut details">
            <h2>SELECTED SHORTCUT</h2>
            <ShortcutDetails key={project && shortcut ? selectionKey(project, shortcut) : 'empty'} state={state} actions={actions} mode={mode} />
          </section>
        </Panel>
      </div>
      {drawerOpen && <Splitter orientation="horizontal" direction={-1} label="Resize terminal pane" value={paneSizes.terminal} limits={terminalLimits}
        onChange={(value) => updatePane('terminal', value)} onCommit={(value) => updatePane('terminal', value)}
        onReset={() => updatePane('terminal', clampSplit(defaultPaneSizes.terminal, terminalLimits()))} />}
      <BottomDrawer open={drawerOpen} id={drawerId} label="Terminal placeholder">
        <h2>TERMINAL / NOT CONNECTED</h2>
        <p>Workspace reserved for a future terminal capability.</p>
        <p>No shell session, command input, execution, or output is connected.</p>
      </BottomDrawer>
    </div>
    <footer className="lb-toolbar">
      <Button aria-expanded={drawerOpen} aria-controls={drawerId} aria-pressed={drawerOpen} onClick={actions.toggleDrawer}>
        <Icon name="terminal" inverse={drawerOpen} />Terminal
      </Button>
      <span className="lb-note">Navigation and local folder access only. No execution or management actions.</span>
    </footer>
  </ApplicationFrame>;
}
