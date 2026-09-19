// @vitest-environment node
import { describe, expect, it, vi } from 'vitest';
import { COMMAND_HISTORY_LIMIT, createLoadbotApplication } from '../frontend/loadbot/application/controller';
import { fixtureAdapter } from '../frontend/loadbot/fixtures/adapter';
import { fixtureSampleForms } from '../frontend/loadbot/fixtures/sampleForms';
import { projectKey, shortcutKey } from '../frontend/loadbot/identity';
import type { LoadbotAdapter, LoadbotProject, ShortcutIdentity } from '../frontend/loadbot/contract';

describe('headless capability and application boundary', () => {
  const adapter = (readInventory: LoadbotAdapter['readInventory'], openProjectFolder: LoadbotAdapter['openProjectFolder'] = vi.fn(async () => {})): LoadbotAdapter => ({
    readInventory,
    readCatalogs: async () => [{ name: 'personal', url: 'test', writable: true, state: 'installed', default: false }, { name: 'community', url: 'test', writable: false, state: 'installed', default: false }, { name: 'one', url: 'test', writable: true, state: 'installed', default: false }, { name: 'two', url: 'test', writable: true, state: 'installed', default: false }, { name: 'three', url: 'test', writable: true, state: 'installed', default: false }],
    openProjectFolder,
    addCatalog: vi.fn(), addProject: vi.fn(), addShortcut: vi.fn(), addRecipeShortcut: vi.fn(), updateRecipeShortcut: vi.fn(),
    chooseProjectFile: vi.fn(), chooseProjectDirectory: vi.fn(), viewShortcutHelp: vi.fn(), deleteShortcuts: vi.fn(), syncCatalog: vi.fn(),
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

  it('owns bounded command history over semantic inventory without producing Activity or adapter side effects', async () => {
    const readInventory = vi.fn(() => fixtureAdapter.readInventory());
    const injected = adapter(readInventory);
    const application = createLoadbotApplication(injected);
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    const activity = application.getSnapshot().activity;
    const commandState = application.getSnapshot().command;

    expect(application.actions.completeCommand('sho', 3)?.candidates.map((candidate) => candidate.value)).toEqual(['shortcuts']);
    expect(application.actions.completeCommand('inspect r', 9)?.candidates.length).toBeGreaterThan(1);
    expect(application.getSnapshot().command).toBe(commandState);
    expect(application.getSnapshot().activity).toBe(activity);
    expect(readInventory).toHaveBeenCalledOnce();

    expect(application.actions.submitCommand('   ')).toBe(false);
    expect(application.actions.submitCommand('projects')).toBe(true);
    expect(application.getSnapshot().command.entries[0]).toMatchObject({
      input: 'projects', result: { kind: 'projects', catalog: 'personal' },
    });
    expect(application.getSnapshot().activity).toBe(activity);
    expect(readInventory).toHaveBeenCalledOnce();
    expect(injected.addProject).not.toHaveBeenCalled();
    expect(injected.syncCatalog).not.toHaveBeenCalled();

    for (let index = 0; index < COMMAND_HISTORY_LIMIT + 4; index++) {
      application.actions.submitCommand(`unknown-${index}`);
    }
    expect(application.getSnapshot().command.entries).toHaveLength(COMMAND_HISTORY_LIMIT);
    expect(application.getSnapshot().command.history).toHaveLength(COMMAND_HISTORY_LIMIT);
    expect(application.getSnapshot().command.history[0]).toBe('unknown-4');
    expect(application.getSnapshot().activity).toBe(activity);
  });

  it('starts from a pre-management configured catalog without registration or migration', async () => {
    const existing: readonly LoadbotProject[] = [{
      catalog: 'existing', tool: 'known-project', entries: [{
        name: 'known-shortcut', path: 'scripts/known.sh', description: 'Existing shortcut', runner: 'sh', source: 'catalog',
      }],
    }];
    const readInventory = vi.fn(async () => existing);
    const readCatalogs = vi.fn(async () => [{
      name: 'existing', url: 'https://example.invalid/existing.git', writable: true,
      state: 'installed' as const, default: true,
    }]);
    const existingAdapter = adapter(readInventory);
    existingAdapter.readCatalogs = readCatalogs;
    const application = createLoadbotApplication(existingAdapter);

    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));

    expect(readCatalogs).toHaveBeenCalledOnce();
    expect(readInventory).toHaveBeenCalledOnce();
    expect(application.getSnapshot().currentCatalog).toBe('existing');
    expect(application.getSnapshot().project?.tool).toBe('known-project');
    expect(application.getSnapshot().shortcut?.name).toBe('known-shortcut');
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
      addRecipeShortcut: vi.fn(), updateRecipeShortcut: vi.fn(), chooseProjectFile: vi.fn(), viewShortcutHelp: vi.fn(),
      chooseProjectDirectory: vi.fn(), deleteShortcuts: vi.fn(),
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
    expect(managed.syncCatalog).toHaveBeenCalledWith('other', expect.any(Function));
    expect(managed.readInventory).toHaveBeenCalledTimes(5);
    expect(new Set(application.getSnapshot().activity.map((entry) => entry.operation))).toEqual(new Set([
      'project-add', 'shortcut-add', 'catalog-add', 'catalog-sync',
    ]));
  });

  it('keeps an existing writable catalog manageable and preserves qualified selection after sync', async () => {
    const projects: readonly LoadbotProject[] = [{ catalog: 'existing', tool: 'project', entries: [
      { name: 'first', path: 'first.sh', source: 'catalog' },
      { name: 'selected', path: 'selected.sh', source: 'personal' },
    ] }];
    const catalogs = [{ name: 'existing', url: 'catalog', writable: true, state: 'installed' as const, default: true }];
    const syncCatalog: LoadbotAdapter['syncCatalog'] = vi.fn(async (_catalog, onActivity) => {
      onActivity?.({ stage: 'validating', catalog: 'existing' });
      onActivity?.({ stage: 'repository-checked', catalog: 'existing' });
      onActivity?.({ stage: 'updating-repository', catalog: 'existing' });
      onActivity?.({ stage: 'current', catalog: 'existing', detail: 'abc1234' });
    });
    const managed: LoadbotAdapter = {
      ...adapter(async () => structuredClone(projects)),
      readCatalogs: vi.fn(async () => structuredClone(catalogs)), syncCatalog,
    };
    const application = createLoadbotApplication(managed);
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    application.actions.selectShortcut(shortcutKey(projects[0].entries[1]));

    expect(await application.actions.syncCatalog()).toBe(true);

    const state = application.getSnapshot();
    expect(state.currentCatalog).toBe('existing');
    expect(state.project?.tool).toBe('project');
    expect(state.shortcut?.name).toBe('selected');
    expect(state.catalogState.status === 'ready' && state.catalogState.catalogs[0]).toMatchObject({
      name: 'existing', writable: true, state: 'installed',
    });
    expect(state.activity.map((entry) => entry.stage)).toEqual([
      'started', 'validating', 'repository-checked', 'updating-repository', 'current',
      'authoritative-reload', 'catalog-state', 'completed',
    ]);
    expect(state.bottomView).toBe('activity');
  });

  it('records sync failure and its authoritative recovery read without false success', async () => {
    const managed = adapter(async () => [{ catalog: 'one', tool: 'project', entries: [] }]);
    managed.syncCatalog = vi.fn(async (_catalog, onActivity) => {
      onActivity?.({ stage: 'validating', catalog: 'one' });
      throw new Error('remote unavailable');
    });
    const application = createLoadbotApplication(managed);
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));

    expect(await application.actions.syncCatalog()).toBe(false);
    expect(application.getSnapshot().activity.map((entry) => [entry.stage, entry.status])).toEqual([
      ['started', 'in-progress'], ['validating', 'in-progress'],
      ['authoritative-reload', 'in-progress'], ['failed', 'error'],
    ]);
    expect(application.getSnapshot().management).toEqual({
      status: 'error', kind: 'sync-catalog', message: 'remote unavailable',
    });
  });

  it('records reload and folder activity and bounds session history', async () => {
    const managed = adapter(async () => [{ catalog: 'one', tool: 'project', entries: [] }]);
    const application = createLoadbotApplication(managed);
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    application.actions.reloadInventory();
    await vi.waitFor(() => expect(application.getSnapshot().activity.at(-1)).toMatchObject({
      operation: 'local-reload', status: 'success',
    }));

    for (let index = 0; index < 260; index++) application.actions.openProjectFolder(projectKey({ catalog: 'one', tool: 'project' }));
    await vi.waitFor(() => expect(application.getSnapshot().activity.at(-1)?.status).toBe('success'));
    expect(application.getSnapshot().activity).toHaveLength(250);
    expect(application.getSnapshot().activity[0].id).toBeGreaterThan(1);
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

  it('owns Recipe draft ordering and saves/reopens authoritative personal Recipes', async () => {
    let projects: LoadbotProject[] = [{ catalog: 'one', tool: 'demo', entries: [] }];
    const managed = adapter(async () => structuredClone(projects));
    managed.addRecipeShortcut = vi.fn(async (input) => {
      projects = [{ ...projects[0]!, entries: [{ name: input.name, description: input.description, recipe: input.recipe, source: 'personal' }] }];
      return { catalog: input.catalog, tool: input.tool, name: input.name };
    });
    managed.updateRecipeShortcut = vi.fn(async (input) => {
      projects = [{ ...projects[0]!, entries: [{ name: input.name, description: input.description, recipe: input.recipe, source: 'personal' }] }];
      return { catalog: input.catalog, tool: input.tool, name: input.name };
    });
    const application = createLoadbotApplication(managed);
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));

    application.actions.openRecipeCreator();
    application.actions.updateRecipeDetails({ name: 'build', description: 'Build it' });
    application.actions.setRecipeTarget('scripts/build.py');
    application.actions.addRecipeParameter('literal');
    const literal = application.getSnapshot().recipeEditor!.draft.arguments[0]!;
    application.actions.updateRecipeParameter(literal.key, { type: 'literal', value: 'build' });
    application.actions.addRecipeParameter('switch');
    const flag = application.getSnapshot().recipeEditor!.draft.arguments[1]!;
    application.actions.updateRecipeParameter(flag.key, { type: 'switch', id: 'release', label: 'Release', value: '--release', default: false }, true);
    application.actions.moveRecipeParameter(flag.key, -1);

    expect(await application.actions.saveRecipe()).toBe(true);
    expect(managed.addRecipeShortcut).toHaveBeenCalledWith(expect.objectContaining({
      catalog: 'one', tool: 'demo', name: 'build', recipe: expect.objectContaining({
        behavior: 'run', program: { type: 'interpreter', runner: 'python' },
        arguments: [{ type: 'project-path', path: 'scripts/build.py' }, expect.objectContaining({ id: 'release' }), { type: 'literal', value: 'build' }],
      }),
    }));
    expect(application.getSnapshot().shortcut).toMatchObject({ name: 'build', recipe: { behavior: 'run' } });
    expect(application.getSnapshot().recipeEditor).toBeUndefined();

    expect(application.actions.openSelectedRecipeEditor()).toBe(true);
    application.actions.setRecipeRunner('bash');
    expect(await application.actions.saveRecipe()).toBe(true);
    expect(managed.updateRecipeShortcut).toHaveBeenCalledWith(expect.objectContaining({ name: 'build', recipe: expect.objectContaining({ behavior: 'run', program: { type: 'interpreter', runner: 'bash' } }) }));
    expect(application.getSnapshot().shortcut).toMatchObject({ recipe: { behavior: 'run' } });
    expect(application.getSnapshot().activity.map((entry) => entry.operation)).toContain('shortcut-update');
  });

  it('keeps cancelled/invalid Recipe drafts out of adapter persistence', async () => {
    const managed = adapter(async () => [{ catalog: 'one', tool: 'demo', entries: [] }]);
    const application = createLoadbotApplication(managed);
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    application.actions.openRecipeCreator();
    expect(await application.actions.saveRecipe()).toBe(false);
    expect(application.getSnapshot().recipeEditor?.errors).toContain('Shortcut name is required.');
    expect(managed.addRecipeShortcut).not.toHaveBeenCalled();
    application.actions.closeRecipeEditor();
    expect(application.getSnapshot().recipeEditor).toBeUndefined();
  });

  it('applies semantic project pickers to author-time paths and treats cancellation as no change', async () => {
    const managed = adapter(async () => [{ catalog: 'one', tool: 'demo', entries: [] }]);
    managed.chooseProjectFile = vi.fn()
      .mockResolvedValueOnce('scripts/tool.py')
      .mockResolvedValueOnce(undefined);
    managed.chooseProjectDirectory = vi.fn(async () => 'scripts/tools');
    const application = createLoadbotApplication(managed);
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    application.actions.openRecipeCreator();
    application.actions.setRecipeTarget('old.py');
    expect(await application.actions.chooseRecipeTarget()).toBe(true);
    expect(application.getSnapshot().recipeEditor?.draft.target).toBe('scripts/tool.py');
    expect(application.getSnapshot().recipeEditor?.draft.runner).toBe('python');
    expect(await application.actions.chooseRecipeWorkingDirectory()).toBe(true);
    expect(application.getSnapshot().recipeEditor?.draft.workingDirectory).toEqual({ type: 'project-relative', path: 'scripts/tools' });
    expect(await application.actions.chooseRecipeTarget()).toBe(false);
    expect(application.getSnapshot().recipeEditor?.draft.target).toBe('scripts/tool.py');
    application.actions.addRecipeParameter('file');
    expect(managed.chooseProjectFile).toHaveBeenCalledTimes(2);
    expect(application.getSnapshot().recipeEditor?.draft.arguments[0]?.value).toMatchObject({ type: 'input', kind: 'file' });
    expect(application.getSnapshot().activity).toEqual([]);
  });

  it('owns help request state, prevents duplicate probes, and clears stale output when invocation changes', async () => {
    let finish!: (value: Awaited<ReturnType<LoadbotAdapter['viewShortcutHelp']>>) => void;
    const managed = adapter(async () => [{ catalog: 'one', tool: 'demo', entries: [] }]);
    managed.viewShortcutHelp = vi.fn(() => new Promise<Awaited<ReturnType<LoadbotAdapter['viewShortcutHelp']>>>((resolve) => { finish = resolve; }));
    const application = createLoadbotApplication(managed);
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    application.actions.openRecipeCreator();

    expect(await application.actions.viewRecipeHelp()).toBe(false);
    expect(application.getSnapshot().recipeEditor?.help).toEqual({ status: 'error', message: 'Choose a target before viewing help.' });
    application.actions.setRecipeTarget('scripts/tool.py');
    expect(application.getSnapshot().recipeEditor?.help).toBeUndefined();
    const first = application.actions.viewRecipeHelp();
    expect(application.getSnapshot().recipeEditor?.help).toEqual({ status: 'loading' });
    expect(await application.actions.viewRecipeHelp()).toBe(false);
    expect(managed.viewShortcutHelp).toHaveBeenCalledOnce();
    expect(managed.viewShortcutHelp).toHaveBeenCalledWith({
      catalog: 'one', tool: 'demo', target: 'scripts/tool.py', runner: 'python',
      workingDirectory: { type: 'project-root' },
    });
    finish({ commandAttempted: ['python', 'scripts/tool.py', '--help'], stdout: 'Usage\n', stderr: '', exitStatus: 0, detectedHelpFlag: '--help' });
    expect(await first).toBe(true);
    expect(application.getSnapshot().recipeEditor?.help).toMatchObject({ status: 'ready', result: { stdout: 'Usage\n' } });

    application.actions.setRecipeRunner('bash');
    expect(application.getSnapshot().recipeEditor?.help).toBeUndefined();
    application.actions.dismissRecipeHelp();
    expect(application.getSnapshot().activity).toEqual([]);
  });

  it('owns atomic personal shortcut selection, confirmation, deletion, and authoritative reload', async () => {
    let projects: LoadbotProject[] = [{ catalog: 'one', tool: 'demo', entries: [
      { name: 'personal-one', path: 'one.sh', source: 'personal' },
      { name: 'shared', path: 'shared.sh', source: 'catalog' },
      { name: 'personal-two', recipe: { version: 1, behavior: 'run', program: { type: 'executable', name: 'cargo' }, working_directory: { type: 'project-root' }, arguments: [] }, source: 'personal' },
    ] }];
    const managed = adapter(async () => structuredClone(projects));
    managed.deleteShortcuts = vi.fn(async (identities: readonly ShortcutIdentity[]) => {
      const names = new Set(identities.map((item) => item.name));
      projects = [{ ...projects[0]!, entries: projects[0]!.entries.filter((item) => !names.has(item.name)) }];
      return identities.length;
    });
    const application = createLoadbotApplication(managed);
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    application.actions.enterShortcutManagement();
    const entries = application.getSnapshot().project!.entries;
    application.actions.toggleShortcutForDeletion(shortcutKey(entries[0]!));
    application.actions.toggleShortcutForDeletion(shortcutKey(entries[1]!));
    application.actions.toggleShortcutForDeletion(shortcutKey(entries[2]!));
    expect(application.getSnapshot().shortcutManagement.selected).toHaveLength(2);
    application.actions.requestSelectedShortcutDeletion();
    expect(application.getSnapshot().shortcutManagement.pendingDelete?.map((item) => item.name)).toEqual(['personal-one', 'personal-two']);
    expect(await application.actions.confirmShortcutDeletion()).toBe(true);
    expect(managed.deleteShortcuts).toHaveBeenCalledOnce();
    expect(managed.deleteShortcuts).toHaveBeenCalledWith([
      { catalog: 'one', tool: 'demo', name: 'personal-one', path: 'one.sh' },
      { catalog: 'one', tool: 'demo', name: 'personal-two', path: undefined },
    ]);
    expect(application.getSnapshot().project?.entries.map((item) => item.name)).toEqual(['shared']);
    expect(application.getSnapshot().shortcutManagement).toEqual({ active: false, selected: [] });
    expect(application.getSnapshot().activity.at(-1)).toMatchObject({ operation: 'shortcut-delete', status: 'success' });
  });

  it('keeps authoritative shortcuts after a delete failure and reports no false success', async () => {
    const projects: LoadbotProject[] = [{ catalog: 'one', tool: 'demo', entries: [
      { name: 'keep-me', path: 'keep.sh', source: 'personal' },
    ] }];
    const managed = adapter(async () => structuredClone(projects));
    managed.deleteShortcuts = vi.fn(async () => { throw new Error('shortcut changed concurrently'); });
    const application = createLoadbotApplication(managed);
    application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));
    application.actions.requestCurrentShortcutDeletion();
    expect(await application.actions.confirmShortcutDeletion()).toBe(false);
    expect(application.getSnapshot().project?.entries.map((item) => item.name)).toEqual(['keep-me']);
    expect(application.getSnapshot().management).toEqual({ status: 'error', kind: 'delete-shortcut', message: 'shortcut changed concurrently' });
    expect(application.getSnapshot().activity.at(-1)).toMatchObject({ operation: 'shortcut-delete', status: 'error' });
  });
});
