import { useEffect, useId, useState } from 'react';
import { projectKey, shortcutKey, type MenuAdapter, type MenuHostCallbacks, type MenuProject, type MenuShortcut, type PreviewField } from '../adapter/types';
import { BottomDrawer, Button, Checkbox, Icon, IconButton, InputControl, MenuList, MenuRow, Panel, PathSelector, StatusDisplay, WindowFrame } from './components';
import { asset } from './theme';

type Values = Record<string, string | boolean>;
function initialValues(fields: readonly PreviewField[]): Values {
  return Object.fromEntries(fields.map((field) => [field.id, field.initialValue ?? (field.kind === 'checkbox' ? false : '')]));
}

function ShortcutDetails({ shortcut }: { shortcut: MenuShortcut }) {
  const fields = shortcut.previewFields ?? [];
  const [values, setValues] = useState<Values>(() => initialValues(fields));
  const statusId = useId();
  const update = (id: string, value: string | boolean) => setValues((current) => ({ ...current, [id]: value }));
  const missing = fields.filter((field) => field.kind !== 'checkbox' && field.required && !String(values[field.id] ?? '').trim());
  return <div className="lb-details">
    <h3>{shortcut.name}</h3>
    {shortcut.description && <p>{shortcut.description}</p>}
    <p className="lb-metadata" title={shortcut.path}>{shortcut.source === 'catalog' ? 'Shared' : 'Personal'} · {shortcut.runner ?? 'Inferred runner'} · {shortcut.path}</p>
    <form onSubmit={(event) => event.preventDefault()} aria-label="Sample shortcut inputs">
      <div className="lb-fields">
        {fields.map((field) => field.kind === 'checkbox'
          ? <Checkbox key={field.id} label={field.label} checked={Boolean(values[field.id])} onChange={(event) => update(field.id, event.target.checked)} />
          : field.kind === 'path'
            ? <PathSelector key={field.id} {...field} value={String(values[field.id] ?? '')} onChange={(value) => update(field.id, value)} describedBy={statusId} />
            : <InputControl key={field.id} label={field.label} value={String(values[field.id] ?? '')} required={field.required} placeholder={field.placeholder}
              aria-invalid={field.required && !String(values[field.id] ?? '').trim() ? true : undefined} aria-describedby={statusId}
              onChange={(event) => update(field.id, event.target.value)} />)}
        {!fields.length && <p className="lb-note">No sample inputs for this shortcut.</p>}
      </div>
      <div className="lb-run">
        <Button disabled aria-describedby={statusId}>RUN SHORTCUT</Button>
        <StatusDisplay id={statusId}>{missing.length
          ? `Input required: ${missing.map((field) => field.label.toLowerCase()).join(', ')}. Execution is not connected.`
          : 'Sample form ready. Execution is not connected.'}</StatusDisplay>
      </div>
    </form>
  </div>;
}

export function LoadbotMenu({ adapter, host }: { adapter: MenuAdapter; host?: MenuHostCallbacks }) {
  const [inventory, setInventory] = useState<{ adapter: MenuAdapter; projects: readonly MenuProject[]; error?: string }>();
  const [projectId, setProjectId] = useState<string>();
  const [shortcutId, setShortcutId] = useState<string>();
  const [drawerOpen, setDrawerOpen] = useState(false);
  const drawerId = useId();
  const unavailableId = useId();
  useEffect(() => {
    let current = true;
    setProjectId(undefined);
    setShortcutId(undefined);
    Promise.resolve().then(() => adapter.readProjects()).then(
      (projects) => { if (current) setInventory({ adapter, projects }); },
      (error: unknown) => { if (current) setInventory({ adapter, projects: [], error: error instanceof Error ? error.message : 'Could not read fixture data.' }); },
    );
    return () => { current = false; };
  }, [adapter]);
  const loaded = inventory?.adapter === adapter;
  const projects = loaded ? inventory.projects : [];
  const project = projects.find((item) => projectKey(item) === projectId) ?? projects[0];
  const shortcut = project?.entries.find((item) => shortcutKey(item) === shortcutId) ?? project?.entries[0];

  return <WindowFrame>
    <header className="lb-header">
      <img className="lb-mascot" src={asset('loadbot.branding.loadbot-header')} alt="" />
      <h1>LOADBOT</h1>
      <span className="lb-fixture-badge">FIXTURE PREVIEW<span>Phase 1 · no live operations</span></span>
      {host?.onClose && <IconButton icon="close" label="Close Loadbot menu" onClick={host.onClose} />}
    </header>
    <div className="lb-workspace">
      <Panel className="lb-sidebar" aria-label="Projects panel">
        <h2>PROJECTS</h2>
        <MenuList label="Projects">
          {projects.map((item) => <MenuRow key={projectKey(item)} icon="folder" selected={item === project}
            title={`${item.catalog}/${item.tool}`} onClick={() => {
              if (item !== project) { setProjectId(projectKey(item)); setShortcutId(undefined); }
            }}>
            {item.tool}<small>{item.catalog}</small>
          </MenuRow>)}
          {!loaded && <StatusDisplay>Loading fixture projects…</StatusDisplay>}
          {loaded && inventory.error && <StatusDisplay>Fixture unavailable: {inventory.error}</StatusDisplay>}
          {loaded && !inventory.error && !projects.length && <StatusDisplay>No fixture projects available.</StatusDisplay>}
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
            title={`${item.name} [${item.source === 'catalog' ? 'shared' : 'personal'}]`} onClick={() => setShortcutId(shortcutKey(item))}>
            {item.name}{project.entries.some((other) => other !== item && other.name === item.name) && <small>[{item.source === 'catalog' ? 'shared' : 'personal'}]</small>}
          </MenuRow>)}
          {project && !project.entries.length && <StatusDisplay>No shortcuts in this fixture project.</StatusDisplay>}
        </MenuList>
        {project && shortcut
          ? <ShortcutDetails key={`${projectKey(project)}:${shortcutKey(shortcut)}`} shortcut={shortcut} />
          : <div className="lb-details"><p>Select a shortcut to see its details.</p></div>}
      </Panel>
    </div>
    <BottomDrawer open={drawerOpen} id={drawerId} />
    <footer className="lb-toolbar">
      <Button aria-expanded={drawerOpen} aria-controls={drawerId} aria-pressed={drawerOpen} onClick={() => setDrawerOpen((open) => !open)}>
        <Icon name="terminal" inverse={drawerOpen} />Terminal
      </Button>
      <Button disabled aria-describedby={unavailableId}>Open project folder</Button>
    </footer>
  </WindowFrame>;
}
