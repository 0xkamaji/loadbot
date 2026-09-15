import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { fixtureMenuDependencies } from '../frontend/hosts/fixtureComposition';
import type { LoadbotAdapter, LoadbotProject } from '../frontend/loadbot/contract';
import { LoadbotMenu } from '../frontend/loadbot/LoadbotMenu';

const projectRows = () => within(screen.getByRole('group', { name: 'Projects' }));
const shortcutRows = () => within(screen.getByRole('group', { name: 'Shortcuts' }));

describe('injected menu outside Tauri', () => {
  const adapter = (projects: readonly LoadbotProject[], open = vi.fn(async () => {})): LoadbotAdapter => ({
    readInventory: async () => projects,
    readCatalogs: async () => [...new Set(projects.map((item) => item.catalog))].map((name, index) => ({ name, url: 'fixture', writable: true, state: 'installed' as const, default: index === 0 })),
    openProjectFolder: open, addCatalog: vi.fn(), addProject: vi.fn(), addShortcut: vi.fn(), syncCatalog: vi.fn(),
  });
  it('changes project/shortcut, resets isolated forms, and preserves state through the drawer', async () => {
    const user = userEvent.setup();
    render(<LoadbotMenu {...fixtureMenuDependencies} />);
    await screen.findByRole('heading', { name: 'Malware triage' });
    expect(screen.getByRole('status')).toHaveTextContent('Input required: input folder');
    await user.click(screen.getByRole('button', { name: 'Use sample input folder' }));
    await user.click(screen.getByRole('checkbox'));
    expect(screen.getByRole('status')).toHaveTextContent('Sample form ready');
    expect(screen.getByRole('button', { name: 'RUN SHORTCUT' })).toBeDisabled();
    expect(screen.getByRole('region', { name: 'Bottom workspace' })).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Terminal' }));
    expect(screen.queryByRole('region', { name: 'Bottom workspace' })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Terminal' }));
    expect(screen.getByRole('region', { name: 'Bottom workspace' })).toBeVisible();
    expect(screen.getByRole('region', { name: 'Bottom workspace' })).not.toContainElement(screen.getByLabelText('Input folder *'));
    expect(screen.getByLabelText('Input folder *')).toHaveValue('samples/');
    expect(screen.getByRole('checkbox')).toBeChecked();
    await user.click(shortcutRows().getByRole('button', { name: 'Export strings' }));
    expect(screen.queryByLabelText('Input folder *')).not.toBeInTheDocument();
    await user.click(projectRows().getByRole('button', { name: 'radio personal' }));
    expect(shortcutRows().queryByRole('button', { name: 'Malware triage' })).not.toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Inspect recording' })).toBeInTheDocument();
    expect(screen.getByLabelText('Recording file *')).toHaveValue('');
    await user.click(screen.getByRole('button', { name: 'Use sample recording file' }));
    await user.type(screen.getByLabelText('Center frequency (MHz) *'), '   ');
    expect(screen.getByRole('status')).toHaveTextContent('Input required: center frequency');
    await user.type(screen.getByLabelText('Center frequency (MHz) *'), '100.5');
    expect(screen.getByRole('status')).toHaveTextContent('Sample form ready');
    await user.click(projectRows().getByRole('button', { name: 're-toolkit personal' }));
    expect(screen.getByLabelText('Input folder *')).toHaveValue('');
    expect(screen.getByRole('checkbox')).not.toBeChecked();
  });

  it('keeps selection distinct from arrow-key focus and supports native activation', async () => {
    const user = userEvent.setup();
    render(<LoadbotMenu {...fixtureMenuDependencies} />);
    const selected = await projectRows().findByRole('button', { name: 're-toolkit personal' });
    selected.focus();
    await user.keyboard('{ArrowDown}');
    expect(projectRows().getByRole('button', { name: 'radio personal' })).toHaveFocus();
    expect(selected).toHaveAttribute('aria-pressed', 'true');
    await user.keyboard('{Enter}');
    expect(screen.getByRole('heading', { name: 'Inspect recording' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Catalog context: personal' }));
    await user.click(screen.getByRole('menuitemradio', { name: /community/ }));
    await projectRows().getByRole('button', { name: 're-toolkit community' }).click();
    const shared = shortcutRows().getByRole('button', { name: 'Export strings [shared]' });
    shared.focus();
    await user.keyboard('{ArrowDown} ');
    expect(screen.getByText('Personal variant; this remains independently selectable.')).toBeInTheDocument();
  });

  it('renders a supplied adapter in an ordinary parent and delegates close to that parent', async () => {
    const injected = adapter([{ catalog: 'test', tool: 'injected', entries: [] }]);
    const close = vi.fn();
    const user = userEvent.setup();
    render(<div style={{ width: 700, height: 500 }}><LoadbotMenu adapter={injected} mode="fixture" host={{ onClose: close }} /></div>);
    expect(await screen.findByRole('button', { name: 'injected test' })).toBeInTheDocument();
    expect(screen.getByText('No shortcuts in this fixture project.')).toBeInTheDocument();
    await user.keyboard('{Escape}');
    expect(close).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Close Loadbot menu' }));
    expect(close).toHaveBeenCalledOnce();
  });

  it('ignores stale adapter responses and displays empty/error results honestly', async () => {
    let resolve!: (projects: readonly LoadbotProject[]) => void;
    const unavailable = vi.fn(async () => { throw new Error('unavailable'); });
    const slow = { ...adapter([], unavailable), readInventory: () => new Promise<readonly LoadbotProject[]>((done) => { resolve = done; }) };
    const empty = adapter([], unavailable);
    const view = render(<LoadbotMenu adapter={slow} mode="fixture" />);
    await act(async () => {});
    view.rerender(<LoadbotMenu adapter={empty} mode="fixture" />);
    await screen.findByText('No fixture projects available.');
    await act(async () => resolve([{ catalog: 'old', tool: 'stale', entries: [] }]));
    expect(screen.queryByRole('button', { name: 'stale old' })).not.toBeInTheDocument();
    view.rerender(<LoadbotMenu adapter={{ ...adapter([], unavailable), readInventory: async () => { throw new Error('Fixture read failed'); } }} mode="fixture" />);
    expect(await screen.findByText('Fixture unavailable: Fixture read failed')).toBeInTheDocument();
  });

  it('opens the row-specific qualified project without changing selection and displays failures', async () => {
    const user = userEvent.setup();
    const projects: readonly LoadbotProject[] = [
      { catalog: 'first', tool: 'selected', entries: [] },
      { catalog: 'first', tool: 'target', entries: [] },
    ];
    const open = vi.fn().mockRejectedValue(new Error('Project directory is missing'));
    render(<LoadbotMenu adapter={adapter(projects, open)} />);
    const first = await projectRows().findByRole('button', { name: 'selected first' });
    await user.click(screen.getByRole('button', { name: 'Open project folder: target (first)' }));
    expect(first).toHaveAttribute('aria-pressed', 'true');
    expect(open).toHaveBeenCalledWith({ catalog: 'first', tool: 'target' });
    expect(await screen.findByText('Project directory is missing')).toBeInTheDocument();
  });

  it('opens, cancels, validates, and submits compact management flows', async () => {
    const user = userEvent.setup();
    let projects: readonly LoadbotProject[] = [{ catalog: 'personal', tool: 'existing', entries: [] }];
    let catalogs = [{ name: 'personal', url: 'catalog', writable: true, state: 'installed' as const, default: true }];
    const addProject = vi.fn(async (input) => {
      projects = [...projects, { catalog: input.catalog, tool: input.name, entries: [] }];
      return { catalog: input.catalog, tool: input.name };
    });
    const addShortcut = vi.fn(async (input) => {
      projects = projects.map((project) => project.tool === input.tool ? { ...project, entries: [{ name: input.name, path: input.path, source: 'personal' as const }] } : project);
      return { catalog: input.catalog, tool: input.tool, name: input.name, path: input.path };
    });
    const managed: LoadbotAdapter = {
      ...adapter(projects), readInventory: async () => projects, readCatalogs: async () => catalogs,
      addProject, addShortcut,
      addCatalog: vi.fn(async (input) => {
        catalogs = [...catalogs, { name: input.name, url: input.url, writable: input.writable, state: 'installed', default: false }];
        return { catalog: input.name };
      }),
      syncCatalog: vi.fn(async () => {}),
    };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'existing personal' });

    await user.click(screen.getByRole('button', { name: '+ ADD PROJECT' }));
    expect(screen.getByRole('dialog', { name: 'Add project' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'CANCEL' }));
    expect(addProject).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: '+ ADD PROJECT' }));
    await user.type(screen.getByLabelText('Project name *'), 'new-project');
    await user.type(screen.getByLabelText('Git repository URL *'), 'https://example.test/new.git');
    await user.click(screen.getByRole('button', { name: 'ADD PROJECT' }));
    expect(await projectRows().findByRole('button', { name: 'new-project personal' })).toHaveAttribute('aria-pressed', 'true');

    await user.click(screen.getByRole('button', { name: '+ ADD SHORTCUT' }));
    expect(screen.getByRole('heading', { name: 'ADD SHORTCUT / new-project' })).toBeInTheDocument();
    await user.type(screen.getByLabelText('Shortcut name *'), 'inspect');
    await user.type(screen.getByLabelText('Repository-relative path *'), 'scripts/inspect.py');
    await user.selectOptions(screen.getByLabelText('Runner (optional)'), 'python');
    await user.click(screen.getByRole('button', { name: 'ADD SHORTCUT' }));
    expect(await shortcutRows().findByRole('button', { name: 'inspect' })).toHaveAttribute('aria-pressed', 'true');
    expect(addShortcut).toHaveBeenCalledWith(expect.objectContaining({ catalog: 'personal', tool: 'new-project', runner: 'python' }));

    await user.click(screen.getByRole('button', { name: 'Catalog context: personal' }));
    await user.click(screen.getByRole('button', { name: 'SYNC CATALOG' }));
    expect(managed.syncCatalog).toHaveBeenCalledWith('personal', expect.any(Function));
    await vi.waitFor(() => expect(screen.getByRole('button', { name: '+ ADD PROJECT' })).toBeEnabled());
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Catalog state: personal · installed · writable');
    await user.click(screen.getByRole('button', { name: 'Catalog context: personal' }));
    await user.click(screen.getByRole('button', { name: '+ ADD CATALOG' }));
    await user.type(screen.getByLabelText('Catalog name *'), 'community');
    await user.type(screen.getByLabelText('Git repository URL *'), 'https://example.test/catalog.git');
    await user.click(screen.getByRole('button', { name: 'ADD AND USE' }));
    expect(await screen.findByRole('button', { name: 'Catalog context: community' })).toBeInTheDocument();
  });
});
