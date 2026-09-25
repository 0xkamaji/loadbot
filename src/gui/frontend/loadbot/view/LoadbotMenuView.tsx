import { useEffect, useId, useRef, useState } from 'react';
import { projectKey, selectionKey, shortcutKey } from '../identity';
import type { LoadbotActions, LoadbotState } from '../application/controller';
import { ApplicationFrame, BottomDrawer, BusyLabel, Button, Icon, IconButton, MenuList, MenuRow, Panel, StatusDisplay, type ShellCallbacks } from '../../ui/components';
import { clampSplit, Splitter } from '../../ui/Splitter';
import mascot from '../../../loadbot-gui-assets/assets/branding/loadbot-header.png';
import { ShortcutDetails } from './ShortcutDetails';
import { ManagementDialogs, type ManagementDialog } from './ManagementDialogs';
import { BottomWorkspace } from './ActivityFeed';
import { decodePaneSizes, defaultPaneSizes, encodePaneSizes, type PaneSizes, type WorkspaceLayoutStore } from './workspaceLayout';
import './menu.css';

/** Pure Loadbot layout: no adapter, fixture, native API, or asynchronous work. */
export function LoadbotMenuView({ state, actions, host, mode, workspaceLayoutStore }: {
  state: LoadbotState; actions: LoadbotActions; host?: ShellCallbacks; mode: 'local' | 'fixture';
  workspaceLayoutStore: WorkspaceLayoutStore;
}) {
  const drawerId = useId();
  const { inventory, project, shortcut, drawerOpen } = state;
  const projects = inventory.status === 'ready'
    ? inventory.projects.filter((item) => (!state.currentCatalog || item.catalog === state.currentCatalog)
      && (state.projectFilter === 'all' || (state.projectFilter === 'installed' ? item.installed !== false : item.installed === false))) : [];
  const catalogs = state.catalogState.status === 'ready' ? state.catalogState.catalogs : [];
  const currentCatalog = catalogs.find((item) => item.name === state.currentCatalog);
  const busy = state.management.status === 'submitting';
  const [catalogMenuOpen, setCatalogMenuOpen] = useState(false);
  const [managementDialog, setManagementDialog] = useState<ManagementDialog>();
  const [paneSizes, setPaneSizes] = useState(defaultPaneSizes);
  const preferredPaneSizes = useRef<PaneSizes>(defaultPaneSizes);
  const preferenceGeneration = useRef(0);
  const workspaceRef = useRef<HTMLDivElement>(null);
  const rightRef = useRef<HTMLElement>(null);
  const mainConsoleRef = useRef<HTMLDivElement>(null);
  const projectLimits = () => {
    const total = workspaceRef.current?.clientWidth || 800;
    const min = Math.min(180, Math.max(120, total - 260));
    return { min, max: Math.max(min, Math.min(560, total - 268)) };
  };
  const shortcutLimits = () => {
    const total = rightRef.current?.clientHeight || 450;
    return { min: 110, max: Math.max(110, Math.min(640, total - 158)) };
  };
  const consoleLimits = () => {
    const total = mainConsoleRef.current?.clientHeight || 580;
    return { min: 88, max: Math.max(88, Math.min(440, total - 258)) };
  };
  const clampToLayout = (sizes: PaneSizes): PaneSizes => ({
    projects: clampSplit(sizes.projects, projectLimits()),
    shortcuts: clampSplit(sizes.shortcuts, shortcutLimits()),
    terminal: clampSplit(sizes.terminal, consoleLimits()),
  });
  const previewPane = (name: keyof PaneSizes, value: number) => {
    preferenceGeneration.current++;
    preferredPaneSizes.current = { ...preferredPaneSizes.current, [name]: value };
    setPaneSizes((current) => ({ ...current, [name]: value }));
  };
  const persistPreferences = () => {
    void workspaceLayoutStore.write(encodePaneSizes(preferredPaneSizes.current)).catch(() => {});
  };
  const resetPane = (name: keyof PaneSizes) => {
    preferenceGeneration.current++;
    preferredPaneSizes.current = { ...preferredPaneSizes.current, [name]: defaultPaneSizes[name] };
    setPaneSizes(clampToLayout(preferredPaneSizes.current));
    persistPreferences();
  };
  useEffect(() => {
    let active = true;
    const generation = preferenceGeneration.current;
    workspaceLayoutStore.read().then((contents) => {
      if (!active || generation !== preferenceGeneration.current) return;
      preferredPaneSizes.current = decodePaneSizes(contents);
      setPaneSizes(clampToLayout(preferredPaneSizes.current));
    }).catch(() => {});
    return () => { active = false; };
  }, [workspaceLayoutStore]);
  useEffect(() => {
    const clampToWindow = () => setPaneSizes((current) => {
      const next = clampToLayout(preferredPaneSizes.current);
      return next.projects === current.projects && next.shortcuts === current.shortcuts && next.terminal === current.terminal
        ? current : next;
    });
    clampToWindow();
    window.addEventListener('resize', clampToWindow);
    const observer = typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(clampToWindow);
    for (const element of [workspaceRef.current, rightRef.current, mainConsoleRef.current]) {
      if (element) observer?.observe(element);
    }
    return () => { window.removeEventListener('resize', clampToWindow); observer?.disconnect(); };
  }, []);
  const catalog = state.currentCatalog ?? project?.catalog ?? 'NO CATALOG';
  return <ApplicationFrame label="Loadbot menu">
    <header className="lb-header">
      <img className="lb-mascot" src={mascot} alt="" />
      <h1>LOADBOT</h1>
      <span className="lb-fixture-badge">{mode === 'fixture' ? 'FIXTURE PREVIEW' : 'LOCAL INVENTORY'}<span>{mode === 'fixture' ? 'Isolated test data' : 'Management workspace'}</span></span>
      <div className="lb-catalog-menu">
        <Button className="lb-catalog-context" disabled={state.catalogState.status !== 'ready'} aria-expanded={catalogMenuOpen}
          onClick={() => setCatalogMenuOpen((open) => !open)} title="Catalog context and management" aria-label={`Catalog context: ${catalog}`}>
          <span>{catalog}</span><Icon name="chevron-down" />
        </Button>
        {catalogMenuOpen && <div className="lb-catalog-popover" role="menu" aria-label="Catalog context">
          {catalogs.map((item) => <button type="button" role="menuitemradio" aria-checked={item.name === state.currentCatalog}
            key={item.name} onClick={() => { actions.selectCatalog(item.name); setCatalogMenuOpen(false); }}>
            <span>{item.name}</span><small>{item.state}{item.writable ? ' · writable' : ' · read-only'}</small>
          </button>)}
          {!catalogs.length && <StatusDisplay>No catalogs configured.</StatusDisplay>}
          {mode === 'local' && <div className="lb-catalog-actions">
            <Button disabled={!currentCatalog || currentCatalog.state !== 'installed' || busy}
              title="Refresh catalog from its configured Git remote"
              onClick={() => { void actions.syncCatalog().then(() => setCatalogMenuOpen(false)); }}>{busy && state.management.kind === 'sync-catalog'
                ? <BusyLabel text="REFRESHING…" /> : 'REFRESH CATALOG'}</Button>
            <Button disabled={busy} onClick={() => { setCatalogMenuOpen(false); actions.clearManagementStatus(); setManagementDialog('add-catalog'); }}>+ ADD CATALOG</Button>
          </div>}
        </div>}
      </div>
      <Button className="lb-reload" disabled={inventory.status === 'loading' || busy} onClick={actions.reloadInventory} title="Reload local Loadbot state">
        {inventory.status === 'loading' ? 'READING…' : 'RELOAD'}
      </Button>
      {host?.onClose && <IconButton icon="close" label="Close Loadbot menu" onClick={host.onClose} />}
    </header>
    <div className={`lb-main-console ${drawerOpen ? '' : 'lb-console-closed'}`} ref={mainConsoleRef}
      style={drawerOpen ? { gridTemplateRows: `minmax(0, 1fr) 8px ${paneSizes.terminal}px` } : undefined}>
      <div className="lb-workspace" ref={workspaceRef} style={{ gridTemplateColumns: `${paneSizes.projects}px 8px minmax(0, 1fr)` }}>
        <Panel className="lb-sidebar" aria-label="Projects panel">
          <div className="lb-section-title"><h2>PROJECTS</h2>{mode === 'local' && <Button className="lb-subtle-action"
            disabled={!currentCatalog?.writable || currentCatalog.state !== 'installed' || busy}
            title={!currentCatalog?.writable ? 'Select an installed writable catalog' : 'Add project to the current catalog'}
            onClick={() => { actions.clearManagementStatus(); setManagementDialog('add-project'); }}>+ ADD PROJECT</Button>}</div>
          <div className="lb-project-filters" role="group" aria-label="Project filter">
            {([['installed', 'Installed'], ['all', 'All'], ['not-installed', 'Not Installed']] as const).map(([value, label]) =>
              <button type="button" key={value} aria-pressed={state.projectFilter === value} onClick={() => actions.selectProjectFilter(value)}>{label}</button>)}
          </div>
          <MenuList label="Projects">
            {projects.map((item) => {
              const id = projectKey(item);
              return <div className="lb-project-row" data-selected={item === project} key={id}>
                <MenuRow className="lb-project-select" icon="arrow-right" selected={item === project}
                  title={`${item.catalog}/${item.tool}`} onClick={() => actions.selectProject(id)}>
                  {item.tool}<small>{item.catalog}{item.installed === false ? ' · AVAILABLE' : ''}</small>
                </MenuRow>
                {item.installed !== false && <IconButton className="lb-open-project" icon="folder" label={`Open project folder: ${item.tool} (${item.catalog})`}
                  title="Open project folder" disabled={state.projectFolder.status === 'opening' && state.projectFolder.projectId === id}
                  onClick={() => actions.openProjectFolder(id)} />}
              </div>;
            })}
            {inventory.status === 'loading' && <StatusDisplay>{mode === 'fixture' ? 'Loading fixture projects…' : 'Reading local Loadbot inventory…'}</StatusDisplay>}
            {inventory.status === 'error' && <StatusDisplay>{mode === 'fixture' ? 'Fixture unavailable: ' : 'Inventory read failed: '}{inventory.message ?? (mode === 'fixture' ? 'Could not read fixture data.' : 'Could not read local Loadbot data.')}</StatusDisplay>}
            {inventory.status === 'ready' && !projects.length && <StatusDisplay>{mode === 'fixture' ? 'No fixture projects available.' : 'No projects in this catalog.'}</StatusDisplay>}
          </MenuList>
          {state.projectFolder.status !== 'idle' && <StatusDisplay>{state.projectFolder.status === 'opening'
            ? 'Opening project folder…' : state.projectFolder.message}</StatusDisplay>}
        </Panel>
        <Splitter orientation="vertical" label="Resize projects pane" value={paneSizes.projects} limits={projectLimits}
          onChange={(value) => previewPane('projects', value)} onCommit={persistPreferences}
          onReset={() => resetPane('projects')} />
        <Panel className="lb-shortcut-panel" aria-label="Shortcut workspace" ref={rightRef}
          style={{ gridTemplateRows: `${paneSizes.shortcuts}px 8px minmax(0, 1fr)` }}>
          <section className="lb-shortcut-list">
            <div className="lb-section-title"><h2>SHORTCUTS <span>/ {project?.tool ?? 'Select a project'}</span></h2>{mode === 'local' && <div className="lb-shortcut-heading-actions">
              {state.shortcutManagement.active ? <Button className="lb-subtle-action" disabled={busy} onClick={actions.exitShortcutManagement}>DONE</Button>
                : project?.entries.some((item) => item.source === 'personal') && <Button className="lb-subtle-action" disabled={busy} onClick={actions.enterShortcutManagement}>MANAGE</Button>}
              {!state.shortcutManagement.active && <Button className="lb-subtle-action" disabled={!project || project.installed === false || busy} onClick={() => {
                actions.clearManagementStatus(); actions.openRecipeCreator(); setManagementDialog('recipe-editor');
              }}>+ ADD SHORTCUT</Button>}
            </div>}</div>
            {mode === 'local' && project && <ProjectActions state={state} actions={actions} />}
            <MenuList label="Shortcuts">
              {project?.entries.map((item) => state.shortcutManagement.active
                ? <label className="lb-manage-shortcut" key={shortcutKey(item)} data-source={item.source}>
                  <input type="checkbox" disabled={item.source !== 'personal' || busy}
                    checked={state.shortcutManagement.selected.includes(shortcutKey(item))}
                    onChange={() => actions.toggleShortcutForDeletion(shortcutKey(item))} />
                  <span>{item.name}<small>{item.source === 'catalog' ? 'CATALOG · READ ONLY' : 'PERSONAL'}</small></span>
                </label>
                : <MenuRow key={shortcutKey(item)} icon="arrow-right" selected={item === shortcut}
                  title={`${item.name} [${item.source === 'catalog' ? 'shared' : 'personal'}]`} onClick={() => actions.selectShortcut(shortcutKey(item))}>
                  {item.name}{project.entries.some((other) => other !== item && other.name === item.name) && <small>[{item.source === 'catalog' ? 'shared' : 'personal'}]</small>}
                </MenuRow>)}
              {!project && inventory.status === 'ready' && projects.length > 0 && <StatusDisplay>Select a project to inspect its shortcuts.</StatusDisplay>}
              {project && !project.entries.length && <StatusDisplay>{mode === 'fixture' ? 'No shortcuts in this fixture project.' : 'No shortcuts in this project.'}</StatusDisplay>}
            </MenuList>
            {state.shortcutManagement.active && <div className="lb-manage-footer"><span>{state.shortcutManagement.selected.length} selected</span>
              <Button className="lb-danger-action" disabled={!state.shortcutManagement.selected.length || busy} onClick={actions.requestSelectedShortcutDeletion}>DELETE SELECTED</Button></div>}
          </section>
          <Splitter orientation="horizontal" label="Resize shortcuts and selected shortcut" value={paneSizes.shortcuts} limits={shortcutLimits}
            onChange={(value) => previewPane('shortcuts', value)} onCommit={persistPreferences}
            onReset={() => resetPane('shortcuts')} />
          <section className="lb-selected-shortcut" aria-label="Selected shortcut details">
            <h2>SELECTED SHORTCUT</h2>
            <ShortcutDetails key={project && shortcut ? selectionKey(project, shortcut) : 'empty'} state={state} actions={actions} mode={mode} />
          </section>
        </Panel>
      </div>
      {drawerOpen && <Splitter orientation="horizontal" direction={-1} label="Resize console pane" value={paneSizes.terminal} limits={consoleLimits}
        onChange={(value) => previewPane('terminal', value)} onCommit={persistPreferences}
        onReset={() => resetPane('terminal')} />}
      <BottomDrawer open={drawerOpen} id={drawerId} label="Bottom workspace">
        <BottomWorkspace state={state} actions={actions} />
      </BottomDrawer>
    </div>
    <footer className="lb-toolbar">
      <Button aria-expanded={drawerOpen} aria-controls={drawerId} aria-pressed={drawerOpen} onClick={actions.toggleDrawer}>
        <Icon name="terminal" inverse={drawerOpen} />Console
      </Button>
      <span className="lb-note">{state.management.status === 'idle' ? 'Management writes are backend-authoritative. Execution is not connected.' : state.management.message}</span>
    </footer>
    {mode === 'local' && <ManagementDialogs dialog={managementDialog} state={state} actions={actions} onClose={() => setManagementDialog(undefined)} />}
  </ApplicationFrame>;
}

function ProjectActions({ state, actions }: { state: LoadbotState; actions: LoadbotActions }) {
  const project = state.project!;
  const id = projectKey(project);
  const busy = state.management.status === 'submitting';
  const [overflow, setOverflow] = useState(false);
  if (project.installed === false) return <div className="lb-project-actions">
    <span className="lb-project-state">AVAILABLE</span>
    <Button disabled={busy} onClick={() => void actions.pullProject()}>{busy && state.management.kind === 'pull-project'
      ? <BusyLabel text="PULLING…" /> : 'PULL'}</Button>
    <span className="lb-metadata">Catalog metadata is available; pull to create the managed local checkout.</span>
  </div>;
  return <div className="lb-project-actions">
    <span className="lb-project-state">INSTALLED</span>
    <Button disabled={busy || state.projectTerminal.status === 'opening'} onClick={() => actions.openProjectTerminal(id)}>OPEN TERMINAL</Button>
    <Button disabled={busy} onClick={() => void actions.updateProject()}>{busy && state.management.kind === 'update-project'
      ? <BusyLabel text="UPDATING…" /> : 'Update from Remote'}</Button>
    <Button disabled={busy || Boolean(state.command.interactive)} onClick={() => void actions.pushProject()}>{busy && state.management.kind === 'push-project'
      ? <BusyLabel text="PUSHING…" /> : 'Push'}</Button>
    <div className="lb-project-overflow">
      <Button aria-label="More project actions" aria-expanded={overflow} disabled={busy} onClick={() => setOverflow((open) => !open)}>…</Button>
      {overflow && <div role="menu" aria-label="Project actions">
        <button type="button" role="menuitem" disabled={busy} onClick={() => { setOverflow(false); actions.requestProjectAction('remove'); }}>Remove</button>
        <button type="button" role="menuitem" disabled={busy} onClick={() => { setOverflow(false); actions.requestProjectAction('reinstall'); }}>Reinstall</button>
      </div>}
    </div>
  </div>;
}
