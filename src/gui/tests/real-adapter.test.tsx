import { render, screen } from '@testing-library/react';
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
  addCatalog: vi.fn(), addProject: vi.fn(), addShortcut: vi.fn(), syncCatalog: vi.fn(),
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

  it('uses the native inventory and layout composition with no sample forms', async () => {
    tauri.invoke.mockClear();
    tauri.invoke.mockImplementation(async (command: string) => command === 'read_loadbot_inventory' ? serializedInventory
      : command === 'read_loadbot_catalogs' ? [{ name: 'alpha', url: 'test', writable: true, state: 'installed', default: true }, { name: 'beta', url: 'test', writable: false, state: 'installed', default: false }]
        : undefined);
    render(<LoadbotMenu {...realMenuDependencies} sampleForms={fixtureMenuDependencies.sampleForms} />);
    expect(await screen.findByRole('button', { name: 'demo alpha' })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith('read_loadbot_inventory');
    expect(tauri.invoke).toHaveBeenCalledWith('read_loadbot_workspace_layout');
    expect(realMenuDependencies).not.toHaveProperty('sampleForms');
    expect(screen.getByText('LOCAL INVENTORY')).toBeInTheDocument();
    expect(screen.queryByText(/fixture|sample form ready/i)).not.toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: 'Loadbot command' })).toBeInTheDocument();
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
    expect(screen.getByRole('textbox', { name: 'Loadbot command' })).toBeInTheDocument();
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
  });

  it('uses only structured semantic native management commands', async () => {
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation(async (command: string, input?: Record<string, unknown>) => {
      if (command === 'read_loadbot_catalogs') return [{ name: 'personal', url: 'repo', writable: true, state: 'installed', default: true }];
      if (command === 'add_loadbot_catalog') return { catalog: input?.name };
      if (command === 'add_loadbot_project') return { catalog: input?.catalog, tool: input?.name };
      if (command === 'add_loadbot_shortcut') return { catalog: input?.catalog, tool: input?.tool, name: input?.name, path: input?.path };
      if (command === 'sync_loadbot_catalog') {
        const channel = input?.onActivity as InstanceType<typeof tauri.Channel>;
        channel.onmessage({ stage: 'repository-checked', catalog: input?.catalog } as never);
        return undefined;
      }
      return [];
    });
    const adapter = createTauriLoadbotAdapter();
    await expect(adapter.readCatalogs()).resolves.toEqual([{ name: 'personal', url: 'repo', writable: true, state: 'installed', default: true }]);
    await adapter.addCatalog({ name: 'other', url: 'other-repo', writable: false });
    await adapter.addProject({ catalog: 'personal', name: 'demo', url: 'tool-repo', commit: false, push: false });
    await adapter.addShortcut({ catalog: 'personal', tool: 'demo', name: 'inspect', path: 'scripts/inspect.py', runner: 'python' });
    const activity = vi.fn();
    await adapter.syncCatalog('personal', activity);
    expect(activity).toHaveBeenCalledWith({ stage: 'repository-checked', catalog: 'personal', detail: undefined });
    expect(tauri.invoke.mock.calls.slice(1)).toEqual([
      ['add_loadbot_catalog', { name: 'other', url: 'other-repo', writable: false }],
      ['add_loadbot_project', { catalog: 'personal', name: 'demo', url: 'tool-repo', commit: false, push: false }],
      ['add_loadbot_shortcut', { catalog: 'personal', tool: 'demo', name: 'inspect', path: 'scripts/inspect.py', runner: 'python' }],
      ['sync_loadbot_catalog', { catalog: 'personal', onActivity: expect.any(tauri.Channel) }],
    ]);
    expect(JSON.stringify(tauri.invoke.mock.calls)).not.toMatch(/shell|powershell\.exe|xdg-open/);
  });

  it('rejects malformed payloads and unavailable hosts rather than substituting fixtures', async () => {
    for (const invalid of [{ projects: [] }, [{ catalog: 'a', tool: 'b' }], [{ catalog: 'a', tool: 'b', entries: [{ name: 'x', path: 'x', source: 'unknown' }] }]]) {
      await expect(createTauriLoadbotAdapter(async () => invalid).readInventory()).rejects.toThrow(/Invalid inventory/);
    }
    tauri.isTauri.mockReturnValueOnce(false);
    await expect(createTauriLoadbotAdapter().readInventory()).rejects.toThrow('requires the native Loadbot application');
  });
});
