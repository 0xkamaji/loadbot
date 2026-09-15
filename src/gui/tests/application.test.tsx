// @vitest-environment node
import { describe, expect, it, vi } from 'vitest';
import { createLoadbotApplication } from '../frontend/loadbot/application/controller';
import { fixtureAdapter } from '../frontend/loadbot/fixtures/adapter';
import { fixtureSampleForms } from '../frontend/loadbot/fixtures/sampleForms';
import { projectKey, shortcutKey } from '../frontend/loadbot/identity';
import type { LoadbotAdapter, LoadbotProject } from '../frontend/loadbot/contract';

describe('headless capability and application boundary', () => {
  const adapter = (readInventory: LoadbotAdapter['readInventory'], openProjectFolder: LoadbotAdapter['openProjectFolder'] = vi.fn(async () => {})): LoadbotAdapter => ({
    readInventory,
    readCatalogs: async () => [{ name: 'personal', url: 'test', writable: true, state: 'installed', default: false }, { name: 'community', url: 'test', writable: false, state: 'installed', default: false }, { name: 'one', url: 'test', writable: true, state: 'installed', default: false }, { name: 'two', url: 'test', writable: true, state: 'installed', default: false }, { name: 'three', url: 'test', writable: true, state: 'installed', default: false }],
    openProjectFolder,
    addCatalog: vi.fn(), addProject: vi.fn(), addShortcut: vi.fn(), syncCatalog: vi.fn(),
  });

  it('returns independent serializable fixture snapshots without widget metadata', async () => {
    const first = await fixtureAdapter.readInventory();
    const next = await fixtureAdapter.readInventory();
    expect(next).toEqual(first);
    expect(first).toHaveLength(5);
    expect(JSON.parse(JSON.stringify(first))).toEqual(first);
    expect(JSON.stringify(first)).not.toMatch(/previewFields|checkbox|sampleValue/);
    Object.assign(first[0], { tool: 'modified by this caller' });
    expect(next[0].tool).toBe('re-toolkit');
    expect((await fixtureAdapter.readInventory())[0].tool).toBe('re-toolkit');
  });

  it('supports selection without React, a host, or sample form configuration', async () => {
    const injected = adapter(vi.fn(() => fixtureAdapter.readInventory()));
    const application = createLoadbotApplication(injected);
    const stop = application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    expect(application.getSnapshot().fields).toEqual([]);
    const projects = await fixtureAdapter.readInventory();
    application.actions.selectCatalog('community');
    application.actions.selectProject(projectKey(projects[4]));
    expect(application.getSnapshot().project?.catalog).toBe('community');
    application.actions.selectShortcut(shortcutKey(projects[4].entries[1]));
    expect(application.getSnapshot().shortcut?.source).toBe('personal');
    application.actions.selectProject('missing');
    application.actions.selectShortcut('missing');
    expect(application.getSnapshot().shortcut?.source).toBe('personal');
    expect(injected.readInventory).toHaveBeenCalledOnce();
    stop();
  });

  it('owns deterministic sample validation and isolation independently of rendering', async () => {
    const application = createLoadbotApplication(fixtureAdapter, fixtureSampleForms);
    const other = createLoadbotApplication(fixtureAdapter, fixtureSampleForms);
    const stop = application.start();
    const stopOther = other.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    await vi.waitFor(() => expect(other.getSnapshot().inventory.status).toBe('ready'));
    expect(application.getSnapshot().missingInputIds).toEqual(['input']);
    application.actions.changeSampleInput('input', '   ');
    application.actions.changeSampleInput('input', true);
    application.actions.changeSampleInput('unknown', 'ignored');
    expect(application.getSnapshot().missingInputIds).toEqual(['input']);
    application.actions.useSamplePath('input');
    application.actions.changeSampleInput('report', true);
    const ready = application.getSnapshot();
    application.actions.selectProject(projectKey(ready.project!));
    application.actions.selectShortcut(shortcutKey(ready.shortcut!));
    expect(application.getSnapshot()).toBe(ready);
    application.actions.toggleDrawer();
    application.actions.toggleDrawer();
    expect(application.getSnapshot().values).toEqual(ready.values);
    expect(application.getSnapshot().missingInputIds).toEqual([]);
    expect(other.getSnapshot().missingInputIds).toEqual(['input']);
    const projects = await fixtureAdapter.readInventory();
    application.actions.selectProject(projectKey(projects[1]));
    expect(application.getSnapshot().values.input).toBe('');
    expect(application.getSnapshot().values).not.toHaveProperty('report');
    expect(application.getSnapshot().missingInputIds).toEqual(['input', 'frequency']);
    stop();
    stopOther();
  });

  it('ignores superseded reads and cleanup responses without a DOM lifecycle', async () => {
    let firstReject!: (error: Error) => void;
    let secondResolve!: (projects: readonly LoadbotProject[]) => void;
    const injected = adapter(vi.fn()
      .mockImplementationOnce(() => new Promise((_, reject) => { firstReject = reject; }))
      .mockImplementationOnce(() => new Promise((resolve) => { secondResolve = resolve; })));
    const application = createLoadbotApplication(injected);
    const listener = vi.fn();
    const unsubscribe = application.subscribe(listener);
    const stopFirst = application.start();
    await Promise.resolve();
    stopFirst();
    const stopSecond = application.start();
    await Promise.resolve();
    secondResolve([]);
    await vi.waitFor(() => expect(application.getSnapshot().inventory).toEqual({ status: 'ready', projects: [] }));
    const ready = application.getSnapshot();
    firstReject(new Error('obsolete error'));
    await Promise.resolve();
    await Promise.resolve();
    expect(application.getSnapshot()).toBe(ready);
    expect(listener).toHaveBeenCalledTimes(3);
    unsubscribe();
    application.actions.toggleDrawer();
    expect(listener).toHaveBeenCalledTimes(3);
    stopSecond();
  });

  it('preserves qualified selections across reload and safely replaces invalid selections', async () => {
    const first: readonly LoadbotProject[] = [
      { catalog: 'one', tool: 'same', entries: [{ name: 'first', path: 'first', source: 'catalog' }, { name: 'keep', path: 'keep', source: 'personal' }] },
      { catalog: 'two', tool: 'same', entries: [] },
    ];
    const preserved: readonly LoadbotProject[] = [
      { catalog: 'one', tool: 'same', entries: [{ name: 'new first', path: 'new', source: 'catalog' }, { name: 'keep', path: 'keep', source: 'personal' }] },
      { catalog: 'two', tool: 'same', entries: [] },
    ];
    const replaced: readonly LoadbotProject[] = [
      { catalog: 'one', tool: 'same', entries: [{ name: 'replacement', path: 'replacement', source: 'catalog' }] },
    ];
    const newProject: readonly LoadbotProject[] = [
      { catalog: 'three', tool: 'different', entries: [{ name: 'new selection', path: 'new-selection', source: 'catalog' }] },
    ];
    const read = vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(preserved)
      .mockResolvedValueOnce(replaced).mockResolvedValueOnce(newProject);
    const application = createLoadbotApplication(adapter(read));
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    application.actions.selectShortcut(shortcutKey(first[0].entries[1]));
    application.actions.reloadInventory();
    await vi.waitFor(() => expect(application.getSnapshot().shortcut?.name).toBe('keep'));
    expect(application.getSnapshot().project).toBe(preserved[0]);
    application.actions.reloadInventory();
    await vi.waitFor(() => expect(application.getSnapshot().shortcut?.name).toBe('replacement'));
    expect(application.getSnapshot().project).toBe(replaced[0]);
    application.actions.reloadInventory();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    expect(application.getSnapshot().project).toBeUndefined();
    application.actions.selectCatalog('three');
    expect(application.getSnapshot().project).toBe(newProject[0]);
    expect(application.getSnapshot().shortcut).toBe(newProject[0].entries[0]);
  });

  it('keeps project selection separate from qualified folder opening and surfaces adapter errors', async () => {
    const projects: readonly LoadbotProject[] = [
      { catalog: 'one', tool: 'first', entries: [] },
      { catalog: 'one', tool: 'second', entries: [] },
    ];
    const open = vi.fn().mockRejectedValue(new Error('Directory is unavailable'));
    const application = createLoadbotApplication(adapter(async () => projects, open));
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    application.actions.openProjectFolder(projectKey(projects[1]));
    expect(application.getSnapshot().project).toBe(projects[0]);
    expect(application.getSnapshot().projectFolder.status).toBe('opening');
    await vi.waitFor(() => expect(application.getSnapshot().projectFolder).toEqual({
      status: 'error', projectId: projectKey(projects[1]), message: 'Directory is unavailable',
    }));
    expect(open).toHaveBeenCalledWith({ catalog: 'one', tool: 'second' });
  });

  it('routes management through qualified adapter operations and reloads authoritative state', async () => {
    let projects: LoadbotProject[] = [{ catalog: 'personal', tool: 'existing', entries: [] }];
    let catalogs = [{ name: 'personal', url: 'catalog', writable: true, state: 'installed' as const, default: true }];
    const managed: LoadbotAdapter = {
      readInventory: vi.fn(async () => structuredClone(projects)),
      readCatalogs: vi.fn(async () => structuredClone(catalogs)),
      openProjectFolder: vi.fn(),
      addCatalog: vi.fn(async (input) => {
        catalogs.push({ name: input.name, url: input.url, writable: input.writable, state: 'installed', default: false });
        return { catalog: input.name };
      }),
      addProject: vi.fn(async (input) => {
        projects.push({ catalog: input.catalog, tool: input.name, entries: [] });
        return { catalog: input.catalog, tool: input.name };
      }),
      addShortcut: vi.fn(async (input) => {
        projects = projects.map((item) => item.catalog === input.catalog && item.tool === input.tool ? { ...item, entries: [
          { name: input.name, path: input.path, description: input.description, runner: input.runner, source: 'personal' as const },
        ] } : item);
        return { catalog: input.catalog, tool: input.tool, name: input.name, path: input.path };
      }),
      syncCatalog: vi.fn(async () => {}),
    };
    const application = createLoadbotApplication(managed);
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));

    expect(await application.actions.addProject({ name: 'new', url: 'repo', commit: false, push: false })).toBe(true);
    expect(managed.addProject).toHaveBeenCalledWith({ catalog: 'personal', name: 'new', url: 'repo', commit: false, push: false });
    expect(application.getSnapshot().project?.tool).toBe('new');
    expect(await application.actions.addShortcut({ name: 'inspect', path: 'scripts/inspect.py', runner: 'python' })).toBe(true);
    expect(managed.addShortcut).toHaveBeenCalledWith({ catalog: 'personal', tool: 'new', name: 'inspect', path: 'scripts/inspect.py', runner: 'python' });
    expect(application.getSnapshot().shortcut?.name).toBe('inspect');

    expect(await application.actions.addCatalog({ name: 'other', url: 'other-repo', writable: false })).toBe(true);
    expect(application.getSnapshot().currentCatalog).toBe('other');
    expect(application.getSnapshot().project).toBeUndefined();
    expect(await application.actions.syncCatalog()).toBe(true);
    expect(managed.syncCatalog).toHaveBeenCalledWith('other');
    expect(managed.readInventory).toHaveBeenCalledTimes(5);
  });

  it('prevents duplicate submissions and never fabricates failed mutations', async () => {
    let finish!: (value: { catalog: string; tool: string }) => void;
    const addProject = vi.fn(() => new Promise<{ catalog: string; tool: string }>((resolve) => { finish = resolve; }));
    const managed = adapter(async () => [{ catalog: 'one', tool: 'existing', entries: [] }]);
    managed.addProject = addProject;
    const application = createLoadbotApplication(managed);
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    const first = application.actions.addProject({ name: 'pending', url: 'repo', commit: false, push: false });
    const duplicate = application.actions.addProject({ name: 'pending', url: 'repo', commit: false, push: false });
    expect(await duplicate).toBe(false);
    expect(addProject).toHaveBeenCalledOnce();
    finish({ catalog: 'one', tool: 'pending' });
    await first;
    expect(application.getSnapshot().project?.tool).toBe('existing');

    managed.addShortcut = vi.fn(async () => { throw new Error('shortcut already exists'); });
    expect(await application.actions.addShortcut({ name: 'duplicate', path: 'run.sh' })).toBe(false);
    expect(application.getSnapshot().management).toEqual({ status: 'error', kind: 'add-shortcut', message: 'shortcut already exists' });
    expect(application.getSnapshot().project?.entries).toEqual([]);
  });
});
