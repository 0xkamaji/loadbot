import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import serializedInventory from '../../../tests/fixtures/gui-inventory.json';
import { createTauriLoadbotAdapter } from '../frontend/hosts/tauriInventoryAdapter';
import { realMenuDependencies } from '../frontend/hosts/realComposition';
import { fixtureMenuDependencies } from '../frontend/hosts/fixtureComposition';
import { LoadbotMenu } from '../frontend/loadbot/LoadbotMenu';
import { createLoadbotApplication } from '../frontend/loadbot/application/controller';
import { projectKey, shortcutKey } from '../frontend/loadbot/identity';

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn(() => true) }));
vi.mock('@tauri-apps/api/core', () => tauri);

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

  it('uses exactly the native read command with no arguments or sample forms', async () => {
    tauri.invoke.mockResolvedValueOnce(serializedInventory);
    render(<LoadbotMenu {...realMenuDependencies} sampleForms={fixtureMenuDependencies.sampleForms} />);
    expect(await screen.findByRole('button', { name: 'demo alpha' })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith('read_loadbot_inventory');
    expect(realMenuDependencies).not.toHaveProperty('sampleForms');
    expect(screen.getByText('LOCAL INVENTORY')).toBeInTheDocument();
    expect(screen.queryByText(/fixture|sample form ready/i)).not.toBeInTheDocument();
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'RUN SHORTCUT' })).toBeDisabled();
    expect(screen.getByText('Execution is not connected.')).toBeInTheDocument();
  });

  it('preserves empty versus failed reads through the unchanged controller and presentation', async () => {
    const empty = createTauriLoadbotAdapter(async () => []);
    const view = render(<LoadbotMenu adapter={empty} />);
    expect(await screen.findByText('No projects with commands or shortcuts in local Loadbot data.')).toBeInTheDocument();
    const failed = createTauriLoadbotAdapter(async () => { throw { message: "catalog 'missing' is not installed" }; });
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
    render(<LoadbotMenu adapter={createTauriLoadbotAdapter(async () => data)} sampleForms={fixtureMenuDependencies.sampleForms} />);
    await screen.findByRole('heading', { name: 'Malware triage' });
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Use sample/ })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'RUN SHORTCUT' })).toBeDisabled();
  });

  it('rejects malformed payloads and unavailable hosts rather than substituting fixtures', async () => {
    for (const invalid of [{ projects: [] }, [{ catalog: 'a', tool: 'b' }], [{ catalog: 'a', tool: 'b', entries: [{ name: 'x', path: 'x', source: 'unknown' }] }]]) {
      await expect(createTauriLoadbotAdapter(async () => invalid).readInventory()).rejects.toThrow(/Invalid inventory/);
    }
    tauri.isTauri.mockReturnValueOnce(false);
    await expect(createTauriLoadbotAdapter().readInventory()).rejects.toThrow('requires the native Loadbot application');
  });
});
