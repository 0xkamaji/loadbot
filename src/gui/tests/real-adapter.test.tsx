import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import serializedInventory from '../../../tests/fixtures/gui-inventory.json';
import { createTauriLoadbotAdapter, type ManagementBridge } from '../frontend/hosts/tauriInventoryAdapter';
import { tauriWorkspaceLayoutStore } from '../frontend/hosts/tauriWorkspaceLayoutStore';
import { realMenuDependencies } from '../frontend/hosts/realComposition';
import { fixtureMenuDependencies } from '../frontend/hosts/fixtureComposition';
import { LoadbotMenu } from '../frontend/loadbot/LoadbotMenu';
import { createLoadbotApplication } from '../frontend/loadbot/application/controller';
import { projectKey, shortcutKey } from '../frontend/loadbot/identity';

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(), isTauri: vi.fn(() => true),
  Channel: class MockChannel<T> { onmessage: (message: T) => void = () => {}; },
}));
vi.mock('@tauri-apps/api/core', () => tauri);
const management = (names: readonly string[] = ['personal']): ManagementBridge => ({
  readCatalogs: async () => names.map((name, index) => ({ name, url: 'test', writable: true, state: 'installed', default: index === 0 })),
  createProjectTerminalLaunch: vi.fn(), pullProject: vi.fn(), updateProject: vi.fn(), removeProject: vi.fn(), reinstallProject: vi.fn(),
  addCatalog: vi.fn(), addProject: vi.fn(), addShortcut: vi.fn(), addRecipeShortcut: vi.fn(), updateRecipeShortcut: vi.fn(),
  chooseProjectFile: vi.fn(), chooseProjectDirectory: vi.fn(), viewShortcutHelp: vi.fn(), deleteShortcuts: vi.fn(), syncCatalog: vi.fn(),
});

describe('one platform-neutral real read adapter', () => {
  it('maps the Rust serialization fixture and preserves all qualified identities and optional fields', async () => {
    const query = vi.fn(async () => serializedInventory);
    const adapter = createTauriLoadbotAdapter(query);
    const projects = await adapter.readInventory();
    expect(projects).toEqual(serializedInventory);
    expect(projectKey(projects[0])).not.toBe(projectKey(projects[1]));
    expect(shortcutKey(projects[0].entries[0])).not.toBe(shortcutKey(projects[0].entries[1]));
    expect(projects[0].entries[1].runner).toBeUndefined();
    expect(projects[0].entries[0].description).toBe('Inspect π data');
    expect(query).toHaveBeenCalledOnce();
    expect(projects).not.toBe(serializedInventory);
  });

  it('does not normalize or reconstruct path strings at the transport boundary', async () => {
    // Type handling must not be OS-dependent. Real command paths are validated by Rust.
    for (const path of ['recipes/inspect file.py', 'C:\\tools\\example.py', '/opt/example.py']) {
      const adapter = createTauriLoadbotAdapter(async () => [{ catalog: 'local', tool: 'tool', entries: [{ name: 'inspect', path, source: 'personal' }] }]);
      expect((await adapter.readInventory())[0].entries[0].path).toBe(path);
    }
  });

  it('maps a semantic Recipe inventory entry without inventing a legacy path', async () => {
    const recipe = {
      version: 1, behavior: 'run',
      program: { type: 'interpreter', runner: 'python' },
      working_directory: { type: 'project-root' },
      arguments: [
        { type: 'project-path', path: 'triage.py' },
        { type: 'input', id: 'sample', label: 'Sample', kind: 'file', required: true },
      ],
    };
    const adapter = createTauriLoadbotAdapter(async () => [{
      catalog: 'personal', tool: 'demo', entries: [{ name: 'triage', recipe, source: 'catalog' }],
    }]);
    const entry = (await adapter.readInventory())[0].entries[0];
    expect(entry.path).toBeUndefined();
    expect(entry.runner).toBeUndefined();
    expect(entry.recipe).toEqual(recipe);
    await expect(createTauriLoadbotAdapter(async () => [{
      catalog: 'personal', tool: 'demo', entries: [{ name: 'mixed', path: 'run.sh', recipe, source: 'catalog' }],
    }]).readInventory()).rejects.toThrow(/expected legacy path or Recipe/);
  });

  it('uses the native inventory and layout composition with no sample forms', async () => {
    tauri.invoke.mockClear();
    tauri.invoke.mockImplementation(async (command: string) => command === 'read_loadbot_inventory' ? serializedInventory
      : command === 'read_loadbot_catalogs' ? [{ name: 'alpha', url: 'test', writable: true, state: 'installed', default: true }, { name: 'beta', url: 'test', writable: false, state: 'installed', default: false }]
        : undefined);
    render(<LoadbotMenu {...realMenuDependencies} sampleForms={fixtureMenuDependencies.sampleForms} />);
    await screen.findByText('No projects in this catalog.');
    fireEvent.click(screen.getByRole('button', { name: 'All' }));
    expect(await screen.findByRole('button', { name: /^demo alpha/ })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith('read_loadbot_inventory');
    expect(tauri.invoke).toHaveBeenCalledWith('read_loadbot_workspace_layout');
    expect(realMenuDependencies).not.toHaveProperty('sampleForms');
    expect(screen.getByText('LOCAL INVENTORY')).toBeInTheDocument();
    expect(screen.queryByText(/fixture|sample form ready/i)).not.toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Loadbot command' })).toBeInTheDocument();
    expect(screen.queryByLabelText('Input folder *')).not.toBeInTheDocument();
    expect(screen.getByText(/Inventory details\. Execution is not connected\./)).toBeInTheDocument();
  });

  it('uses the native GUI-local layout document rather than browser storage', async () => {
    tauri.invoke.mockClear();
    tauri.invoke.mockResolvedValueOnce('{"version":1,"projects":321,"shortcuts":222,"terminal":123}');
    await expect(tauriWorkspaceLayoutStore.read()).resolves.toContain('"projects":321');
    await tauriWorkspaceLayoutStore.write('{"version":1}');
    expect(tauri.invoke.mock.calls).toEqual([
      ['read_loadbot_workspace_layout'],
      ['write_loadbot_workspace_layout', { contents: '{"version":1}' }],
    ]);
  });

  it('preserves empty versus failed reads through the unchanged controller and presentation', async () => {
    const empty = createTauriLoadbotAdapter(async () => [], undefined, management([]));
    const view = render(<LoadbotMenu adapter={empty} />);
    expect(await screen.findByText('No projects in this catalog.')).toBeInTheDocument();
    const failed = createTauriLoadbotAdapter(async () => { throw { message: "catalog 'missing' is not installed" }; }, undefined, management([]));
    view.rerender(<LoadbotMenu adapter={failed} />);
    expect(await screen.findByText("Inventory read failed: catalog 'missing' is not installed")).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /re-toolkit/ })).not.toBeInTheDocument();
    const app = createLoadbotApplication(empty);
    const stop = app.start();
    await vi.waitFor(() => expect(app.getSnapshot().inventory).toEqual({ status: 'ready', projects: [] }));
    stop();
  });

  it('ignores demo inputs in local mode even when real identities match a sample selection', async () => {
    const data = await fixtureMenuDependencies.adapter.readInventory();
    render(<LoadbotMenu adapter={createTauriLoadbotAdapter(async () => data, undefined, management(['personal', 'community']))} sampleForms={fixtureMenuDependencies.sampleForms} />);
    await screen.findByRole('heading', { name: 'Malware triage' });
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Loadbot command' })).toBeInTheDocument();
    expect(screen.queryByLabelText('Input folder *')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Use sample/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'RUN SHORTCUT' })).not.toBeInTheDocument();
  });

  it('opens a project by qualified identity through exactly one semantic native command', async () => {
    tauri.invoke.mockClear();
    tauri.invoke.mockResolvedValueOnce(undefined);
    const adapter = createTauriLoadbotAdapter();
    await adapter.openProjectFolder({ catalog: 'catalog with spaces', tool: 'tool; $(not-a-shell)' });
    expect(tauri.invoke).toHaveBeenCalledWith('open_loadbot_project', {
      catalog: 'catalog with spaces', tool: 'tool; $(not-a-shell)',
    });
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
    tauri.invoke.mockRejectedValueOnce({ message: 'project is not installed' });
    await expect(adapter.openProjectFolder({ catalog: 'x', tool: 'y' })).rejects.toThrow('project is not installed');
    tauri.invoke.mockRejectedValueOnce({ kind: 'cancelled', message: 'operation cancelled' });
    const cancelled = await adapter.updateProject?.({ catalog: 'x', tool: 'y' }).catch((error) => error);
    expect(cancelled).toMatchObject({ message: 'operation cancelled', kind: 'cancelled' });
  });

  it('uses only structured semantic native management commands', async () => {
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation(async (command: string, input?: Record<string, unknown>) => {
      if (command === 'read_loadbot_catalogs') return [{ name: 'personal', url: 'repo', writable: true, state: 'installed', default: true }];
      if (command === 'add_loadbot_catalog') return { catalog: input?.name };
      if (command === 'add_loadbot_project') return { catalog: input?.catalog, tool: input?.name };
      if (command === 'create_loadbot_project_terminal_launch') return { launchId: 'terminal-launch', label: 'Project terminal — demo' };
      if (['pull_loadbot_project', 'update_loadbot_project', 'remove_loadbot_project', 'reinstall_loadbot_project'].includes(command)) {
        const channel = input?.onActivity as InstanceType<typeof tauri.Channel>;
        channel.onmessage({ kind: 'log', stream: 'command', text: `git ${command}` } as never);
        channel.onmessage({
          stage: command === 'update_loadbot_project' ? 'fetching-and-updating'
            : command === 'remove_loadbot_project' ? 'removing-checkout' : 'cloning-project',
          catalog: input?.catalog,
          tool: input?.tool,
        } as never);
        return { catalog: input?.catalog, tool: input?.tool };
      }
      if (command === 'add_loadbot_shortcut') return { catalog: input?.catalog, tool: input?.tool, name: input?.name, path: input?.path };
      if (command === 'add_loadbot_recipe_shortcut' || command === 'update_loadbot_recipe_shortcut') return { catalog: input?.catalog, tool: input?.tool, name: input?.name, path: null };
      if (command === 'choose_loadbot_project_file') return 'scripts/tool.py';
      if (command === 'choose_loadbot_project_directory') return null;
      if (command === 'view_loadbot_shortcut_help') return {
        commandAttempted: ['python', 'scripts/tool.py', '--help'], stdout: 'Usage\n', stderr: '', exitStatus: 0, detectedHelpFlag: '--help',
      };
      if (command === 'delete_loadbot_shortcuts') return (input?.identities as unknown[]).length;
      if (command === 'sync_loadbot_catalog') {
        const channel = input?.onActivity as InstanceType<typeof tauri.Channel>;
        channel.onmessage({ kind: 'log', stream: 'stderr', text: 'remote diagnostic' } as never);
        channel.onmessage({ stage: 'repository-checked', catalog: input?.catalog } as never);
        return undefined;
      }
      return [];
    });
    const adapter = createTauriLoadbotAdapter();
    await expect(adapter.readCatalogs()).resolves.toEqual([{ name: 'personal', url: 'repo', writable: true, state: 'installed', default: true }]);
    await adapter.addCatalog({ name: 'other', url: 'other-repo', writable: false });
    await adapter.addProject({ catalog: 'personal', name: 'demo', url: 'tool-repo', commit: false, push: false });
    await expect(adapter.createProjectTerminalLaunch?.({ catalog: 'personal', tool: 'demo' })).resolves.toEqual({
      launchId: 'terminal-launch', label: 'Project terminal — demo',
    });
    const projectActivity = vi.fn();
    await adapter.pullProject?.({ catalog: 'personal', tool: 'demo' }, projectActivity);
    await adapter.updateProject?.({ catalog: 'personal', tool: 'demo' }, projectActivity);
    await adapter.removeProject?.({ catalog: 'personal', tool: 'demo' }, projectActivity);
    await adapter.reinstallProject?.({ catalog: 'personal', tool: 'demo' }, projectActivity);
    await adapter.addShortcut({ catalog: 'personal', tool: 'demo', name: 'inspect', path: 'scripts/inspect.py', runner: 'python' });
    const recipe = { version: 1, behavior: 'run' as const, program: { type: 'executable' as const, name: 'cargo' }, working_directory: { type: 'project-root' as const }, arguments: [{ type: 'literal' as const, value: 'build' }] };
    await adapter.addRecipeShortcut({ catalog: 'personal', tool: 'demo', name: 'build', recipe });
    await adapter.updateRecipeShortcut({ catalog: 'personal', tool: 'demo', name: 'build', description: 'Build it', recipe: { ...recipe, behavior: 'launch' } });
    await expect(adapter.chooseProjectFile({ catalog: 'personal', tool: 'demo' })).resolves.toBe('scripts/tool.py');
    await expect(adapter.chooseProjectDirectory({ catalog: 'personal', tool: 'demo' })).resolves.toBeUndefined();
    const helpRequest = { catalog: 'personal', tool: 'demo', target: 'scripts/tool.py', runner: 'python' as const, workingDirectory: { type: 'project-root' as const } };
    await expect(adapter.viewShortcutHelp(helpRequest)).resolves.toEqual({
      commandAttempted: ['python', 'scripts/tool.py', '--help'], stdout: 'Usage\n', stderr: '', exitStatus: 0, detectedHelpFlag: '--help',
    });
    await expect(adapter.deleteShortcuts([{ catalog: 'personal', tool: 'demo', name: 'build' }])).resolves.toBe(1);
    const activity = vi.fn();
    await adapter.syncCatalog('personal', activity);
    expect(activity).toHaveBeenCalledWith({ stage: 'repository-checked', catalog: 'personal', detail: undefined });
    expect(activity).toHaveBeenCalledWith({ kind: 'log', stream: 'stderr', text: 'remote diagnostic' });
    expect(projectActivity.mock.calls.filter(([event]) => !('kind' in event)).map(([event]) => event.stage)).toEqual([
      'cloning-project', 'fetching-and-updating', 'removing-checkout', 'cloning-project',
    ]);
    expect(projectActivity.mock.calls.filter(([event]) => 'kind' in event).map(([event]) => event.text)).toEqual([
      'git pull_loadbot_project', 'git update_loadbot_project', 'git remove_loadbot_project', 'git reinstall_loadbot_project',
    ]);
    expect(tauri.invoke.mock.calls.slice(1)).toEqual([
      ['add_loadbot_catalog', { name: 'other', url: 'other-repo', writable: false }],
      ['add_loadbot_project', { catalog: 'personal', name: 'demo', url: 'tool-repo', revision: undefined, commit: false, push: false }],
      ['create_loadbot_project_terminal_launch', { catalog: 'personal', tool: 'demo' }],
      ['pull_loadbot_project', { catalog: 'personal', tool: 'demo', onActivity: expect.any(tauri.Channel) }],
      ['update_loadbot_project', { catalog: 'personal', tool: 'demo', onActivity: expect.any(tauri.Channel) }],
      ['remove_loadbot_project', { catalog: 'personal', tool: 'demo', onActivity: expect.any(tauri.Channel) }],
      ['reinstall_loadbot_project', { catalog: 'personal', tool: 'demo', onActivity: expect.any(tauri.Channel) }],
      ['add_loadbot_shortcut', { catalog: 'personal', tool: 'demo', name: 'inspect', path: 'scripts/inspect.py', description: undefined, runner: 'python' }],
      ['add_loadbot_recipe_shortcut', { catalog: 'personal', tool: 'demo', name: 'build', recipe }],
      ['update_loadbot_recipe_shortcut', { catalog: 'personal', tool: 'demo', name: 'build', description: 'Build it', recipe: { ...recipe, behavior: 'launch' } }],
      ['choose_loadbot_project_file', { catalog: 'personal', tool: 'demo' }],
      ['choose_loadbot_project_directory', { catalog: 'personal', tool: 'demo' }],
      ['view_loadbot_shortcut_help', { request: helpRequest }],
      ['delete_loadbot_shortcuts', { identities: [{ catalog: 'personal', tool: 'demo', name: 'build' }] }],
      ['sync_loadbot_catalog', { catalog: 'personal', onActivity: expect.any(tauri.Channel) }],
    ]);
    expect(JSON.stringify(tauri.invoke.mock.calls)).not.toMatch(/shell|powershell\.exe|xdg-open/);
  });

  it('bridges opaque interactive capabilities, ordered UTF-8 output, input, and termination', async () => {
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation(async (command: string, input?: Record<string, unknown>) => {
      if (command === 'start_loadbot_interactive_session') {
        const channel = input?.onEvent as InstanceType<typeof tauri.Channel>;
        channel.onmessage({ kind: 'output', sessionId: 'session-1', bytes: [0xe2] } as never);
        channel.onmessage({ kind: 'output', sessionId: 'session-1', bytes: [0x82, 0xac, 0x0a] } as never);
        channel.onmessage({ kind: 'exited', sessionId: 'session-1', code: 0, signal: null, cancelled: false } as never);
        return { sessionId: 'session-1', processId: 'process-1', osProcessId: 42 };
      }
      return undefined;
    });
    const adapter = createTauriLoadbotAdapter();
    const events = vi.fn();
    await expect(adapter.startInteractiveSession?.({ launchId: 'backend-token', label: 'Prompt' }, events))
      .resolves.toEqual({ sessionId: 'session-1', processId: 'process-1', osProcessId: 42 });
    expect(events.mock.calls.map(([event]) => event)).toEqual([
      { kind: 'output', sessionId: 'session-1', text: '€\n' },
      { kind: 'exited', sessionId: 'session-1', code: 0, signal: undefined, cancelled: false },
    ]);
    await adapter.sendInteractiveInput?.('session-1', 'opaque value\r');
    await adapter.terminateInteractiveSession?.('session-1');
    expect(tauri.invoke.mock.calls).toEqual([
      ['start_loadbot_interactive_session', {
        request: { launchId: 'backend-token' }, onEvent: expect.any(tauri.Channel),
      }],
      ['send_loadbot_interactive_input', { sessionId: 'session-1', input: 'opaque value\r' }],
      ['terminate_loadbot_interactive_session', { sessionId: 'session-1' }],
    ]);
    expect(tauri.invoke.mock.calls[0][1]).not.toHaveProperty('program');
    expect(tauri.invoke.mock.calls[0][1]).not.toHaveProperty('arguments');
  });

  it('delivers a backend-issued Push launch without exposing its executable or arguments', async () => {
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation(async (command: string, input?: Record<string, unknown>) => {
      expect(command).toBe('push_loadbot_project');
      const channel = input?.onActivity as InstanceType<typeof tauri.Channel>;
      channel.onmessage({
        kind: 'interactive-launch', launchId: 'backend-push-token', label: 'Git push — demo',
      } as never);
      return { catalog: 'personal', tool: 'demo' };
    });
    const adapter = createTauriLoadbotAdapter();
    const activity = vi.fn();

    await expect(adapter.pushProject?.({ catalog: 'personal', tool: 'demo' }, activity))
      .resolves.toEqual({ catalog: 'personal', tool: 'demo' });
    expect(activity).toHaveBeenCalledWith({
      kind: 'interactive-launch', launchId: 'backend-push-token', label: 'Git push — demo',
    });
    const request = tauri.invoke.mock.calls[0][1] as Record<string, unknown>;
    expect(request).toEqual({ catalog: 'personal', tool: 'demo', onActivity: expect.any(tauri.Channel) });
    expect(JSON.stringify(request)).not.toMatch(/program|arguments|shell/);
  });

  it('transports only semantic Push inspection and Commit & Push fields', async () => {
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation(async (command: string, input?: Record<string, unknown>) => {
      const channel = input?.onActivity as InstanceType<typeof tauri.Channel>;
      if (command === 'inspect_loadbot_project_push') {
        channel.onmessage({ stage: 'inspecting-repository', catalog: 'personal', tool: 'demo' } as never);
        return {
          changedFiles: [{ path: 'a file.txt', originalPath: 'old.txt', status: 'renamed' }],
          commitsAhead: false,
        };
      }
      expect(command).toBe('commit_and_push_loadbot_project');
      channel.onmessage({ stage: 'creating-commit', catalog: 'personal', tool: 'demo' } as never);
      return { catalog: 'personal', tool: 'demo' };
    });
    const adapter = createTauriLoadbotAdapter();
    const activity = vi.fn();
    await expect(adapter.inspectProjectPush?.({ catalog: 'personal', tool: 'demo' }, activity)).resolves.toEqual({
      changedFiles: [{ path: 'a file.txt', originalPath: 'old.txt', status: 'renamed' }], commitsAhead: false,
    });
    await expect(adapter.commitAndPushProject?.({
      catalog: 'personal', tool: 'demo', selectedPaths: ['a file.txt'], commitMessage: 'Rename safely; $(opaque)',
    }, activity)).resolves.toEqual({ catalog: 'personal', tool: 'demo' });
    expect(activity).toHaveBeenCalledWith({ stage: 'inspecting-repository', catalog: 'personal', tool: 'demo' });
    expect(activity).toHaveBeenCalledWith({ stage: 'creating-commit', catalog: 'personal', tool: 'demo' });
    const commitRequest = tauri.invoke.mock.calls[1][1] as Record<string, unknown>;
    expect(commitRequest).toEqual({
      request: {
        catalog: 'personal', tool: 'demo', selectedPaths: ['a file.txt'], commitMessage: 'Rename safely; $(opaque)',
      },
      onActivity: expect.any(tauri.Channel),
    });
    expect(JSON.stringify(commitRequest)).not.toMatch(/program|arguments|shell/);
  });

  it('rejects malformed payloads and unavailable hosts rather than substituting fixtures', async () => {
    for (const invalid of [{ projects: [] }, [{ catalog: 'a', tool: 'b' }], [{ catalog: 'a', tool: 'b', entries: [{ name: 'x', path: 'x', source: 'unknown' }] }]]) {
      await expect(createTauriLoadbotAdapter(async () => invalid).readInventory()).rejects.toThrow(/Invalid inventory/);
    }
    tauri.isTauri.mockReturnValueOnce(false);
    await expect(createTauriLoadbotAdapter().readInventory()).rejects.toThrow('requires the native Loadbot application');
  });
});
