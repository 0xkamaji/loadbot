import { act, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { fixtureMenuDependencies } from '../frontend/hosts/fixtureComposition';
import type { LoadbotAdapter, LoadbotProject, ProjectDirectoryListing, ProjectIdentity } from '../frontend/loadbot/contract';
import { LoadbotMenu } from '../frontend/loadbot/LoadbotMenu';

const projectRows = () => within(screen.getByRole('group', { name: 'Projects' }));
const deferred = <T,>() => {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
};

describe('injected menu outside Tauri', () => {
  const adapter = (projects: readonly LoadbotProject[], open = vi.fn(async () => {})): LoadbotAdapter => ({
    readInventory: async () => projects,
    readCatalogs: async () => [...new Set(projects.map((item) => item.catalog))].map((name, index) => ({ name, backend: 'git' as const, url: 'fixture', writable: true, state: 'installed' as const, default: index === 0 })),
    openCatalogFolder: vi.fn(), openProjectFolder: open, addCatalog: vi.fn(), createCatalog: vi.fn(), addProject: vi.fn(), addShortcut: vi.fn(), addRecipeShortcut: vi.fn(), updateRecipeShortcut: vi.fn(),
    chooseProjectFile: vi.fn(), chooseProjectDirectory: vi.fn(), viewShortcutHelp: vi.fn(), deleteShortcuts: vi.fn(), syncCatalog: vi.fn(),
  });
  it('presents Files as the intentional project workspace without shortcut controls', async () => {
    render(<LoadbotMenu {...fixtureMenuDependencies} />);
    await screen.findByRole('button', { name: 're-toolkit personal' });
    expect(await screen.findByRole('heading', { name: 'FILES / re-toolkit' })).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Project files workspace' })).toBeInTheDocument();
    expect(screen.queryByRole('tab', { name: 'SHORTCUTS' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '+ ADD SHORTCUT' })).not.toBeInTheDocument();
    expect(screen.queryByRole('region', { name: 'Selected shortcut details' })).not.toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Bottom workspace' })).toBeVisible();
    expect(screen.getByRole('tab', { name: 'COMMAND' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'ACTIVITY' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'TERMINAL' })).toBeInTheDocument();
  });

  it('browses only qualified project directories and resets at a new project root', async () => {
    const user = userEvent.setup();
    const projects: LoadbotProject[] = [
      { catalog: 'personal', tool: 'alpha', installed: true, entries: [] },
      { catalog: 'personal', tool: 'beta', installed: true, entries: [] },
    ];
    const readProjectDirectory: NonNullable<LoadbotAdapter['readProjectDirectory']> = vi.fn(
      async (_project: ProjectIdentity, relativePath: string): Promise<ProjectDirectoryListing> => relativePath
        ? { path: relativePath, parent: '', entries: [{ name: 'run.sh', path: `${relativePath}/run.sh`, kind: 'file', size: 12 }] }
        : { path: '', entries: [{ name: 'scripts', path: 'scripts', kind: 'directory' }] },
    );
    render(<LoadbotMenu adapter={{ ...adapter(projects), readProjectDirectory }} />);
    await screen.findByRole('button', { name: 'alpha personal' });

    expect(screen.getByRole('heading', { name: 'FILES / alpha' })).toBeInTheDocument();
    expect(screen.queryByRole('tab', { name: 'SHORTCUTS' })).not.toBeInTheDocument();
    await screen.findByRole('button', { name: /scripts/i });
    expect(screen.queryByRole('region', { name: 'Selected shortcut details' })).not.toBeInTheDocument();
    expect(readProjectDirectory).toHaveBeenLastCalledWith({ catalog: 'personal', tool: 'alpha' }, '');
    await user.click(screen.getByRole('button', { name: /scripts/i }));
    expect(await screen.findByText('run.sh')).toBeInTheDocument();
    expect(readProjectDirectory).toHaveBeenLastCalledWith({ catalog: 'personal', tool: 'alpha' }, 'scripts');
    expect(screen.queryByText('Parent directory')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Go to parent directory' }));
    await screen.findByRole('button', { name: /scripts/i });
    expect(readProjectDirectory).toHaveBeenLastCalledWith({ catalog: 'personal', tool: 'alpha' }, '');

    await user.click(projectRows().getByRole('button', { name: 'beta personal' }));
    expect(screen.getByRole('heading', { name: 'FILES / beta' })).toBeInTheDocument();
    await screen.findByRole('button', { name: /scripts/i });
    expect(readProjectDirectory).toHaveBeenLastCalledWith({ catalog: 'personal', tool: 'beta' }, '');
  });

  it('submits structured Loadbot commands and navigates session history separately from Activity', async () => {
    const user = userEvent.setup();
    render(<LoadbotMenu {...fixtureMenuDependencies} />);
    await screen.findByRole('button', { name: 're-toolkit personal' });
    const input = screen.getByRole('combobox', { name: 'Loadbot command' }) as HTMLInputElement;
    await user.type(input, 'projects{Enter}');
    expect(screen.getByRole('tabpanel', { name: 'Command' })).toHaveTextContent('rotbot');
    expect(screen.getByRole('tabpanel', { name: 'Command' })).toHaveTextContent('re-toolkit');
    expect(input).toHaveValue('');
    expect(input).toHaveFocus();
    await user.type(input, 'help{Enter}');
    expect(screen.getByRole('tabpanel', { name: 'Command' })).toHaveTextContent('inspect <project>');
    expect(screen.getByRole('tabpanel', { name: 'Command' })).not.toHaveTextContent('shortcut');
    await user.keyboard('{ArrowUp}');
    expect(input).toHaveValue('help');
    await user.keyboard('{ArrowUp}');
    expect(input).toHaveValue('projects');
    await user.keyboard('{ArrowDown}');
    expect(input).toHaveValue('help');
    await user.keyboard('{ArrowDown}');
    expect(input).toHaveValue('');
    await user.keyboard('{Enter}');
    await user.click(screen.getByRole('tab', { name: 'ACTIVITY' }));
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('No activity yet.');
  });

  it('completes registered commands and semantic identities without submitting or creating activity', async () => {
    const user = userEvent.setup();
    render(<LoadbotMenu {...fixtureMenuDependencies} />);
    await screen.findByRole('button', { name: 're-toolkit personal' });
    const input = screen.getByRole('combobox', { name: 'Loadbot command' }) as HTMLInputElement;

    await user.type(input, 'sho{Tab}');
    expect(input).toHaveValue('sho');
    expect(screen.queryByRole('listbox', { name: 'Command completions' })).not.toBeInTheDocument();
    expect(screen.queryByText('Available commands:')).not.toBeInTheDocument();

    await user.clear(input);
    await user.type(input, 'wat{Tab}');
    expect(input).toHaveValue('wat');
    expect(screen.queryByRole('listbox', { name: 'Command completions' })).not.toBeInTheDocument();
    input.setSelectionRange(2, 2);
    await user.keyboard('{ArrowLeft}');
    expect(input.selectionStart).toBe(1);
    await user.keyboard('{ArrowRight}');
    expect(input.selectionStart).toBe(2);

    await user.clear(input);
    await user.type(input, 'inspect r{Tab}');
    const candidates = screen.getAllByRole('option');
    expect(candidates.map((candidate) => candidate.textContent)).toEqual([
      're-toolkit', 'radio', 'rotbot', 'community/research-tools-with-a-long-project-name', 'community/re-toolkit',
    ]);
    expect(candidates[0]).toHaveAttribute('aria-selected', 'true');
    await user.keyboard('{Shift>}{Tab}{/Shift}');
    expect(candidates.at(-1)).toHaveAttribute('aria-selected', 'true');
    await user.keyboard('{Tab}');
    expect(candidates[0]).toHaveAttribute('aria-selected', 'true');
    await user.keyboard('{Tab}');
    expect(candidates[1]).toHaveAttribute('aria-selected', 'true');
    await user.keyboard('{Shift>}{Tab}{/Shift}');
    expect(candidates[0]).toHaveAttribute('aria-selected', 'true');
    await user.keyboard('{ArrowLeft}');
    expect(candidates.at(-1)).toHaveAttribute('aria-selected', 'true');
    await user.keyboard('{ArrowRight}');
    expect(candidates[0]).toHaveAttribute('aria-selected', 'true');
    await user.keyboard('{Escape}');
    expect(input).toHaveValue('inspect r');
    expect(screen.queryByRole('listbox', { name: 'Command completions' })).not.toBeInTheDocument();

    await user.keyboard('{Tab}{Enter}');
    expect(input).toHaveValue('inspect re-toolkit');
    expect(screen.queryByText('Project', { selector: '.lb-command-output > p' })).not.toBeInTheDocument();
    await user.keyboard('{Enter}');
    expect(input).toHaveValue('');
    expect(screen.getByText('Project', { selector: '.lb-command-output > p' })).toBeInTheDocument();

    await user.type(input, 'inspect r{Tab}a');
    expect(input).toHaveValue('inspect radio');
    expect(screen.queryByRole('listbox', { name: 'Command completions' })).not.toBeInTheDocument();
    await user.keyboard('{ArrowUp}');
    expect(input).toHaveValue('inspect re-toolkit');

    await user.clear(input);
    await user.type(input, 'inspect r{Tab}');
    input.setSelectionRange(8, 8);
    await user.keyboard('{Delete}');
    expect(input).toHaveValue('inspect ');
    expect(screen.getByRole('listbox', { name: 'Command completions' })).toBeInTheDocument();
    await user.type(input, 'r{Backspace}');
    expect(input).toHaveValue('inspect ');

    await user.click(screen.getByRole('tab', { name: 'ACTIVITY' }));
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('No activity yet.');
  });

  it('uses rendered rows for vertical completion navigation and bounds the candidate field', async () => {
    const user = userEvent.setup();
    render(<LoadbotMenu {...fixtureMenuDependencies} />);
    await screen.findByRole('button', { name: 're-toolkit personal' });
    const input = screen.getByRole('combobox', { name: 'Loadbot command' });
    await user.type(input, 'inspect r{Tab}');
    const candidates = screen.getAllByRole('option');
    const positions = [
      [0, 0, 80], [100, 0, 180], [200, 0, 280], [0, 24, 80], [120, 24, 220],
    ];
    candidates.forEach((candidate, index) => vi.spyOn(candidate, 'getBoundingClientRect').mockReturnValue({
      x: positions[index][0], y: positions[index][1], left: positions[index][0], top: positions[index][1],
      right: positions[index][2], bottom: positions[index][1] + 18, width: positions[index][2] - positions[index][0],
      height: 18, toJSON: () => ({}),
    }));

    await user.keyboard('{ArrowRight}{ArrowDown}');
    expect(candidates[4]).toHaveAttribute('aria-selected', 'true');
    await user.keyboard('{ArrowDown}');
    expect(candidates[1]).toHaveAttribute('aria-selected', 'true');
    const field = screen.getByRole('listbox', { name: 'Command completions' });
    expect(field).toHaveClass('lb-command-completions');
    expect(field).toBeInTheDocument();
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
    expect(screen.getByRole('heading', { name: 'FILES / radio' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Catalog context: personal' }));
    await user.click(screen.getByRole('menuitemradio', { name: /community/ }));
    await projectRows().getByRole('button', { name: 're-toolkit community' }).click();
    expect(screen.getByRole('heading', { name: 'FILES / re-toolkit' })).toBeInTheDocument();
  });

  it('renders a supplied adapter in an ordinary parent and delegates close to that parent', async () => {
    const injected = adapter([{ catalog: 'test', tool: 'injected', entries: [] }]);
    const close = vi.fn();
    const user = userEvent.setup();
    render(<div style={{ width: 700, height: 500 }}><LoadbotMenu adapter={injected} mode="fixture" host={{ onClose: close }} /></div>);
    expect(await screen.findByRole('button', { name: 'injected test' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'FILES / injected' })).toBeInTheDocument();
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
    let catalogs = [{ name: 'personal', backend: 'git' as const, url: 'catalog', writable: true, state: 'installed' as const, default: true }];
    const addProject = vi.fn(async (input) => {
      projects = [...projects, { catalog: input.catalog, tool: input.name, entries: [] }];
      return { catalog: input.catalog, tool: input.name };
    });
    const managed: LoadbotAdapter = {
      ...adapter(projects), readInventory: async () => projects, readCatalogs: async () => catalogs,
      addProject,
      addCatalog: vi.fn(async (input) => {
        catalogs = [...catalogs, { name: input.name, backend: 'git', url: input.url, writable: input.writable, state: 'installed', default: false }];
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

    await user.click(screen.getByRole('button', { name: 'Catalog context: personal' }));
    await user.click(screen.getByRole('button', { name: 'REFRESH CATALOG' }));
    expect(managed.syncCatalog).toHaveBeenCalledWith('personal', expect.any(Function));
    await vi.waitFor(() => expect(screen.getByRole('button', { name: '+ ADD PROJECT' })).toBeEnabled());
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Catalog state: personal · installed · writable');
    await user.click(screen.getByRole('button', { name: 'Catalog context: personal' }));
    expect(screen.getByRole('button', { name: '+ CREATE NEW CATALOG' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '+ ADD EXISTING CATALOG' }));
    await user.type(screen.getByLabelText('Catalog name *'), 'community');
    await user.type(screen.getByLabelText('Git repository URL *'), 'https://example.test/catalog.git');
    await user.click(screen.getByRole('button', { name: 'ADD AND USE' }));
    expect(await screen.findByRole('button', { name: 'Catalog context: community' })).toBeInTheDocument();
  });

  it('creates a catalog with explicit commit and push choices and keeps failed input editable', async () => {
    const user = userEvent.setup();
    let catalogs = [{ name: 'personal', backend: 'git' as const, url: 'catalog', writable: true, state: 'installed' as const, default: true }];
    const pending = deferred<{ catalog: string }>();
    const createCatalog = vi.fn(async (input) => {
      const result = await pending.promise;
      catalogs = [...catalogs, { name: input.name, backend: input.backend, url: input.backend === 'git' ? input.url : undefined, writable: true, state: 'installed', default: false }];
      return result;
    });
    const managed: LoadbotAdapter = {
      ...adapter([{ catalog: 'personal', tool: 'existing', entries: [] }]),
      readCatalogs: async () => catalogs,
      createCatalog,
    };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'existing personal' });

    await user.click(screen.getByRole('button', { name: 'Catalog context: personal' }));
    await user.click(screen.getByRole('button', { name: '+ CREATE NEW CATALOG' }));
    expect(screen.getByRole('dialog', { name: 'Create new catalog' })).toBeInTheDocument();
    const submit = screen.getByRole('button', { name: 'CREATE AND USE' });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByLabelText('Catalog name *'), { target: { value: ' new-catalog ' } });
    await user.click(screen.getByRole('radio', { name: 'Git-backed' }));
    fireEvent.change(screen.getByLabelText('Empty Git repository URL *'), { target: { value: ' /tmp/empty.git ' } });
    expect(submit).toBeEnabled();
    const push = screen.getByRole('checkbox', { name: 'Push initial catalog commit' });
    expect(push).toBeDisabled();
    await user.click(screen.getByRole('checkbox', { name: 'Commit initial catalog.toml' }));
    await user.click(push);
    await user.click(submit);

    expect(createCatalog).toHaveBeenCalledWith({ name: 'new-catalog', backend: 'git', url: '/tmp/empty.git', commit: true, push: true });
    expect(screen.getByRole('button', { name: 'CREATING…' })).toBeDisabled();
    pending.resolve({ catalog: 'new-catalog' });
    expect(await screen.findByRole('button', { name: 'Catalog context: new-catalog' })).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Catalog context: new-catalog' }));
    await user.click(screen.getByRole('button', { name: '+ CREATE NEW CATALOG' }));
    fireEvent.change(screen.getByLabelText('Catalog name *'), { target: { value: 'retry-me' } });
    await user.click(screen.getByRole('radio', { name: 'Git-backed' }));
    fireEvent.change(screen.getByLabelText('Empty Git repository URL *'), { target: { value: '/tmp/retry.git' } });
    managed.createCatalog = vi.fn(async () => { throw new Error('remote is not empty'); });
    await user.click(screen.getByRole('button', { name: 'CREATE AND USE' }));
    expect(await within(screen.getByRole('dialog', { name: 'Create new catalog' })).findByRole('status'))
      .toHaveTextContent('remote is not empty');
    expect(screen.getByLabelText('Catalog name *')).toHaveValue('retry-me');
    expect(screen.getByLabelText('Empty Git repository URL *')).toHaveValue('/tmp/retry.git');
  });

  it('creates local-only catalogs without rendering Git-only fields', async () => {
    const user = userEvent.setup();
    let catalogs = [{ name: 'personal', backend: 'git' as const, url: 'catalog', writable: true, state: 'installed' as const, default: true }];
    const createCatalog = vi.fn(async (input) => {
      catalogs = [...catalogs, {
        name: input.name, backend: input.backend, url: input.backend === 'git' ? input.url : undefined,
        writable: true, state: 'installed', default: false,
      }];
      return { catalog: input.name };
    });
    const managed: LoadbotAdapter = {
      ...adapter([{ catalog: 'personal', tool: 'existing', entries: [] }]),
      readCatalogs: async () => catalogs,
      createCatalog,
    };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'existing personal' });

    await user.click(screen.getByRole('button', { name: 'Catalog context: personal' }));
    await user.click(screen.getByRole('button', { name: '+ CREATE NEW CATALOG' }));
    expect(screen.getByRole('radio', { name: 'Local only' })).toBeChecked();
    expect(screen.queryByLabelText('Empty Git repository URL *')).not.toBeInTheDocument();
    expect(screen.queryByRole('checkbox', { name: /initial catalog/ })).not.toBeInTheDocument();
    await user.type(screen.getByLabelText('Catalog name *'), 'lab');
    await user.click(screen.getByRole('button', { name: 'CREATE AND USE' }));

    expect(createCatalog).toHaveBeenCalledWith({ name: 'lab', backend: 'local' });
    expect(await screen.findByRole('button', { name: 'Catalog context: lab' })).toBeInTheDocument();
  });

  it('manages local catalogs without offering refresh and opens by catalog identity', async () => {
    const user = userEvent.setup();
    const openCatalogFolder = vi.fn(async () => {});
    const managed: LoadbotAdapter = {
      ...adapter([{ catalog: 'lab', tool: 'existing', entries: [] }]),
      readCatalogs: async () => [{
        name: 'lab', backend: 'local', writable: true, state: 'installed', default: true,
      }],
      openCatalogFolder,
    };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'existing lab' });

    await user.click(screen.getByRole('button', { name: 'Catalog context: lab' }));
    expect(screen.queryByRole('button', { name: 'REFRESH CATALOG' })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'MANAGE CATALOG' }));
    const dialog = screen.getByRole('dialog', { name: 'Manage catalog lab' });
    expect(dialog).toHaveTextContent('Local only');
    expect(dialog).toHaveTextContent('installed');
    expect(within(dialog).getByRole('button', { name: 'OPEN TERMINAL' })).toBeEnabled();
    expect(within(dialog).queryByRole('button', { name: 'REFRESH CATALOG' })).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'OPEN FOLDER' }));
    expect(openCatalogFolder).toHaveBeenCalledWith({ catalog: 'lab' });
  });

  it('confirms three distinct catalog removal scopes and recovers selection from authoritative reads', async () => {
    const user = userEvent.setup();
    let catalogs = ['alpha', 'beta', 'gamma'].map((name, index) => ({
      name, backend: 'git' as const, url: `${name}-remote`, writable: true, state: 'installed' as const, default: index === 0,
    }));
    let projects: readonly LoadbotProject[] = catalogs.map(({ name }) => ({
      catalog: name, tool: `${name}-tool`, installed: true, entries: [],
    }));
    const remove = (catalog: string) => {
      catalogs = catalogs.filter((item) => item.name !== catalog);
      projects = projects.filter((item) => item.catalog !== catalog);
      return { catalog };
    };
    const inspectCatalogDeletion: NonNullable<LoadbotAdapter['inspectCatalogDeletion']> = vi.fn(async ({ catalog }) => ({
      catalog, managedTools: [{ tool: `${catalog}-tool` }, { tool: `${catalog}-extra` }],
    }));
    const inspectLocalCatalogDeletion: NonNullable<LoadbotAdapter['inspectLocalCatalogDeletion']> = vi.fn(async ({ catalog }) => ({
      catalog, managedTools: [],
    }));
    const unregisterCatalog: NonNullable<LoadbotAdapter['unregisterCatalog']> = vi.fn(async ({ catalog }) => remove(catalog));
    const deleteLocalCatalog: NonNullable<LoadbotAdapter['deleteLocalCatalog']> = vi.fn(async ({ catalog }) => remove(catalog));
    const deleteCatalogWithManagedTools: NonNullable<LoadbotAdapter['deleteCatalogWithManagedTools']> = vi.fn(async ({ catalog }) => remove(catalog));
    const managed: LoadbotAdapter = {
      ...adapter(projects), readInventory: async () => projects, readCatalogs: async () => catalogs,
      inspectCatalogDeletion, inspectLocalCatalogDeletion, unregisterCatalog, deleteLocalCatalog, deleteCatalogWithManagedTools,
    };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'alpha-tool alpha' });

    await user.click(screen.getByRole('button', { name: 'Catalog context: alpha' }));
    await user.click(screen.getByRole('button', { name: 'MANAGE CATALOG' }));
    await user.click(screen.getByRole('button', { name: 'REMOVE REGISTRATION ONLY' }));
    let unregister = screen.getByRole('dialog', { name: 'remove registration alpha' });
    expect(unregister).toHaveTextContent('local catalog directory and every managed tool checkout remain');
    expect(inspectCatalogDeletion).not.toHaveBeenCalled();
    expect(inspectLocalCatalogDeletion).not.toHaveBeenCalled();
    await user.click(within(unregister).getByRole('button', { name: 'CANCEL' }));
    expect(unregisterCatalog).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'REMOVE REGISTRATION ONLY' }));
    unregister = screen.getByRole('dialog', { name: 'remove registration alpha' });
    await user.click(within(unregister).getByRole('button', { name: 'REMOVE REGISTRATION' }));
    expect(unregisterCatalog).toHaveBeenCalledWith({ catalog: 'alpha' });
    expect(await screen.findByRole('button', { name: 'Catalog context: beta' })).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Catalog context: beta' }));
    await user.click(screen.getByRole('button', { name: 'MANAGE CATALOG' }));
    await user.click(screen.getByRole('button', { name: 'DELETE LOCAL CATALOG, KEEP TOOLS' }));
    const localDelete = await screen.findByRole('dialog', { name: 'delete local catalog beta' });
    expect(localDelete).toHaveTextContent('Managed tool checkouts remain on disk');
    await user.click(within(localDelete).getByRole('button', { name: 'DELETE LOCAL CATALOG' }));
    expect(inspectLocalCatalogDeletion).toHaveBeenCalledWith({ catalog: 'beta' });
    expect(inspectCatalogDeletion).not.toHaveBeenCalled();
    expect(deleteLocalCatalog).toHaveBeenCalledWith({ catalog: 'beta' });
    expect(await screen.findByRole('button', { name: 'Catalog context: gamma' })).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Catalog context: gamma' }));
    await user.click(screen.getByRole('button', { name: 'MANAGE CATALOG' }));
    await user.click(screen.getByRole('button', { name: 'DELETE CATALOG & MANAGED TOOLS' }));
    const fullDelete = await screen.findByRole('dialog', { name: 'delete catalog & managed tools gamma' });
    expect(fullDelete).toHaveTextContent('MANAGED TOOL CHECKOUTS TO DELETE (2)');
    expect(fullDelete).toHaveTextContent('gamma-tool');
    expect(inspectCatalogDeletion).toHaveBeenCalledWith({ catalog: 'gamma' });
    await user.click(within(fullDelete).getByRole('button', { name: 'DELETE CATALOG & TOOLS' }));
    expect(deleteCatalogWithManagedTools).toHaveBeenCalledWith({ catalog: 'gamma' });
    await vi.waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    const emptyCatalogs = screen.getByRole('button', { name: 'Catalog context: NO CATALOG' });
    expect(emptyCatalogs).toBeEnabled();
    await user.click(emptyCatalogs);
    expect(screen.getByText('No catalogs configured.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '+ ADD EXISTING CATALOG' })).toBeEnabled();
    expect(screen.getByRole('button', { name: '+ CREATE NEW CATALOG' })).toBeEnabled();
    expect(screen.getByText('No projects in this catalog.')).toBeInTheDocument();
  });

  it('keeps a failed catalog deletion recoverable and preserves current selection', async () => {
    const user = userEvent.setup();
    const deleteCatalogWithManagedTools: NonNullable<LoadbotAdapter['deleteCatalogWithManagedTools']> = vi.fn(
      async () => { throw new Error('working tree has local changes'); },
    );
    const managed: LoadbotAdapter = {
      ...adapter([{ catalog: 'personal', tool: 'demo', installed: true, entries: [] }]),
      inspectCatalogDeletion: vi.fn(async () => ({ catalog: 'personal', managedTools: [{ tool: 'demo' }] })),
      deleteCatalogWithManagedTools,
    };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'demo personal' });
    await user.click(screen.getByRole('button', { name: 'Catalog context: personal' }));
    await user.click(screen.getByRole('button', { name: 'MANAGE CATALOG' }));
    await user.click(screen.getByRole('button', { name: 'DELETE CATALOG & MANAGED TOOLS' }));
    const dialog = await screen.findByRole('dialog', { name: 'delete catalog & managed tools personal' });
    await user.click(within(dialog).getByRole('button', { name: 'DELETE CATALOG & TOOLS' }));

    expect(await within(dialog).findByRole('status')).toHaveTextContent('working tree has local changes');
    expect(within(dialog).getByRole('button', { name: 'DELETE CATALOG & TOOLS' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Catalog context: personal' })).toBeInTheDocument();
    expect(projectRows().getByRole('button', { name: 'demo personal' })).toHaveAttribute('aria-pressed', 'true');
    await user.click(within(dialog).getByRole('button', { name: 'CANCEL' }));
    expect(screen.getByRole('dialog', { name: 'Manage catalog personal' })).toBeInTheDocument();
  });

  it('connects an installed writable local catalog to an empty Git repository', async () => {
    const user = userEvent.setup();
    let backend: 'local' | 'git' = 'local';
    const connectCatalog: NonNullable<LoadbotAdapter['connectCatalog']> = vi.fn(async (input, onActivity) => {
      onActivity?.({ stage: 'validating', catalog: input.name });
      onActivity?.({ stage: 'preparing-repository', catalog: input.name });
      onActivity?.({ kind: 'log', stream: 'command', text: 'git init' });
      onActivity?.({ stage: 'updating-catalog', catalog: input.name });
      backend = 'git';
      return { catalog: input.name };
    });
    const managed: LoadbotAdapter = {
      ...adapter([{ catalog: 'lab', tool: 'existing', entries: [] }]),
      readCatalogs: async () => [{
        name: 'lab', backend, url: backend === 'git' ? '/tmp/lab.git' : undefined,
        writable: true, state: 'installed', default: true,
      }],
      connectCatalog,
    };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'existing lab' });

    await user.click(screen.getByRole('button', { name: 'Catalog context: lab' }));
    await user.click(screen.getByRole('button', { name: 'MANAGE CATALOG' }));
    await user.click(within(screen.getByRole('dialog', { name: 'Manage catalog lab' })).getByRole('button', { name: 'CONNECT TO REPO' }));
    const dialog = screen.getByRole('dialog', { name: 'Connect catalog lab to Git' });
    expect(dialog).toHaveTextContent('must already exist and contain no refs');
    const push = within(dialog).getByRole('checkbox', { name: 'Push catalog commit' });
    expect(push).toBeDisabled();
    await user.type(within(dialog).getByLabelText('Empty Git repository URL *'), ' /tmp/lab.git ');
    await user.click(within(dialog).getByRole('checkbox', { name: 'Commit catalog.toml' }));
    await user.click(push);
    await user.click(within(dialog).getByRole('button', { name: 'CONNECT TO REPO' }));

    expect(connectCatalog).toHaveBeenCalledWith({ name: 'lab', url: '/tmp/lab.git', commit: true, push: true }, expect.any(Function));
    await vi.waitFor(() => expect(screen.queryByRole('dialog', { name: 'Connect catalog lab to Git' })).not.toBeInTheDocument());
    expect(screen.getAllByText('Catalog lab connected to Git.')).not.toHaveLength(0);
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Preparing Git repository: lab');
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Updating local catalog registration: lab');
    await user.click(screen.getByRole('button', { name: 'Catalog context: lab' }));
    expect(screen.getByRole('menu', { name: 'Catalog context' })).toHaveTextContent('Git-backed');
  });

  it('shows catalog refresh progress and restores its label after completion', async () => {
    const user = userEvent.setup();
    const pending = deferred<void>();
    const managed: LoadbotAdapter = {
      ...adapter([{ catalog: 'personal', tool: 'demo', installed: true, entries: [] }]),
      syncCatalog: vi.fn((_catalog, onActivity) => {
        onActivity?.({ stage: 'repository-checked', catalog: 'personal' });
        onActivity?.({ kind: 'log', stream: 'command', text: 'git fetch origin' });
        onActivity?.({ kind: 'log', stream: 'stderr', text: 'From github.com:0xkamaji/loadbot-catalog' });
        onActivity?.({ stage: 'updating-repository', catalog: 'personal' });
        return pending.promise;
      }),
    };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'demo personal' });

    await user.click(screen.getByRole('button', { name: 'Catalog context: personal' }));
    await user.click(screen.getByRole('button', { name: 'REFRESH CATALOG' }));
    const refreshing = screen.getByRole('button', { name: 'REFRESHING…' });
    expect(refreshing).toBeDisabled();
    const activity = screen.getByRole('tabpanel', { name: 'Activity' });
    expect(activity).toHaveTextContent('Refresh Catalog started');
    expect(activity).toHaveTextContent('RUNNING');
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Configured catalog repository verified');
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Updating catalog from its configured Git remote');
    const verbose = screen.getByText('Verbose logs');
    expect(screen.getByText(/git fetch origin/)).not.toBeVisible();
    await user.click(verbose);
    expect(screen.getByText(/git fetch origin/)).toBeVisible();
    expect(screen.getByText(/From github.com:0xkamaji\/loadbot-catalog/)).toBeVisible();
    expect(document.body).not.toHaveTextContent(/\d+%/);

    await act(async () => pending.resolve());
    await user.click(screen.getByRole('button', { name: 'Catalog context: personal' }));
    expect(screen.getByRole('button', { name: 'REFRESH CATALOG' })).toBeEnabled();
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Catalog personal refreshed.');
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('COMPLETED');
    await user.click(screen.getByText('Refresh Catalog: personal'));
    expect(within(screen.getByRole('tabpanel', { name: 'Activity' })).getByText('Catalog personal refreshed.')).not.toBeVisible();
  });

  it('renders and routes the installed and available project lifecycle controls', async () => {
    const user = userEvent.setup();
    const projects: readonly LoadbotProject[] = [
      { catalog: 'personal', tool: 'installed', installed: true, entries: [] },
      { catalog: 'personal', tool: 'available', installed: false, entries: [] },
    ];
    const pending = deferred<{ catalog: string; tool: string }>();
    const pushProject: NonNullable<LoadbotAdapter['pushProject']> = vi.fn(() => pending.promise);
    const managed: LoadbotAdapter = {
      ...adapter(projects),
      readInventory: async () => structuredClone(projects),
      pushProject,
    };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'installed personal' });

    expect(screen.getByRole('button', { name: 'Update from Remote' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Push' })).toBeEnabled();
    expect(screen.queryByRole('button', { name: 'OPEN FOLDER' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Open project folder: installed (personal)' })).toBeEnabled();
    await user.click(screen.getByRole('button', { name: 'More project actions' }));
    expect(screen.getByRole('menuitem', { name: 'Reinstall' })).toBeEnabled();
    expect(screen.getByRole('menuitem', { name: 'Remove' })).toBeEnabled();

    await user.click(screen.getByRole('button', { name: 'Push' }));
    expect(pushProject).toHaveBeenCalledWith({ catalog: 'personal', tool: 'installed' }, expect.any(Function));
    expect(screen.getByRole('button', { name: 'PUSHING…' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Update from Remote' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'OPEN TERMINAL' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'More project actions' })).toBeDisabled();
    expect(screen.getByRole('menuitem', { name: 'Reinstall' })).toBeDisabled();
    expect(screen.getByRole('menuitem', { name: 'Remove' })).toBeDisabled();

    await act(async () => pending.resolve({ catalog: 'personal', tool: 'installed' }));
    expect(await screen.findByRole('button', { name: 'Push' })).toBeEnabled();
    await user.click(screen.getByRole('button', { name: 'Not Installed' }));
    expect(await screen.findByRole('button', { name: 'PULL' })).toBeEnabled();
    expect(screen.queryByRole('button', { name: 'Update from Remote' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Push' })).not.toBeInTheDocument();
  });

  it('uses one Push entry point for the structured Commit & Push dialog', async () => {
    const user = userEvent.setup();
    const project: LoadbotProject = { catalog: 'personal', tool: 'demo', installed: true, entries: [] };
    const commitAndPushProject: NonNullable<LoadbotAdapter['commitAndPushProject']> = vi.fn(async (input) => input);
    const managed: LoadbotAdapter = {
      ...adapter([project]),
      inspectProjectPush: vi.fn(async () => ({
        changedFiles: [
          { path: 'src/main.rs', status: 'modified' as const },
          { path: 'notes/new file.txt', status: 'added' as const },
        ],
        commitsAhead: false,
      })),
      pushProject: vi.fn(async (identity) => identity),
      commitAndPushProject,
    };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'demo personal' });
    expect(screen.getAllByRole('button', { name: 'Push' })).toHaveLength(1);

    await user.click(screen.getByRole('button', { name: 'Push' }));
    const dialog = await screen.findByRole('dialog', { name: 'Commit & Push' });
    expect(within(dialog).getByText('demo')).toBeInTheDocument();
    const modified = within(dialog).getByRole('checkbox', { name: /modified src\/main\.rs/i });
    const added = within(dialog).getByRole('checkbox', { name: /added notes\/new file\.txt/i });
    expect(modified).toBeChecked();
    expect(added).toBeChecked();
    expect(within(dialog).getByRole('button', { name: 'Commit & Push' })).toBeDisabled();
    await user.type(within(dialog).getByLabelText('Commit message *'), 'Ship selected change');
    await user.click(modified);
    await user.click(added);
    expect(within(dialog).getByText('Select at least one changed file.')).toBeInTheDocument();
    expect(within(dialog).getByRole('button', { name: 'Commit & Push' })).toBeDisabled();
    await user.click(added);
    expect(within(dialog).getByRole('button', { name: 'Commit & Push' })).toBeEnabled();
    await user.click(within(dialog).getByRole('button', { name: 'CANCEL' }));
    expect(screen.queryByRole('dialog', { name: 'Commit & Push' })).not.toBeInTheDocument();
    expect(commitAndPushProject).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: 'Push' }));
    const reopened = await screen.findByRole('dialog', { name: 'Commit & Push' });
    expect(within(reopened).getAllByRole('checkbox')).toHaveLength(2);
    expect(within(reopened).getAllByRole('checkbox').every((checkbox) => (checkbox as HTMLInputElement).checked)).toBe(true);
    await user.type(within(reopened).getByLabelText('Commit message *'), 'Commit both files');
    await user.click(within(reopened).getByRole('button', { name: 'Commit & Push' }));
    await vi.waitFor(() => expect(commitAndPushProject).toHaveBeenCalledWith({
      catalog: 'personal', tool: 'demo', selectedPaths: ['src/main.rs', 'notes/new file.txt'], commitMessage: 'Commit both files',
    }, expect.any(Function)));
    expect(screen.queryByRole('dialog', { name: 'Commit & Push' })).not.toBeInTheDocument();
  });

  it('shows project lifecycle busy labels, real stages, completion, and failure', async () => {
    const user = userEvent.setup();
    let projects: LoadbotProject[] = [
      { catalog: 'personal', tool: 'installed', installed: true, entries: [] },
      { catalog: 'personal', tool: 'available', installed: false, entries: [] },
    ];
    const pull = deferred<{ catalog: string; tool: string }>();
    const update = deferred<{ catalog: string; tool: string }>();
    const reinstall = deferred<{ catalog: string; tool: string }>();
    const remove = deferred<{ catalog: string; tool: string }>();
    const managed: LoadbotAdapter = {
      ...adapter(projects),
      readInventory: async () => structuredClone(projects),
      pullProject: vi.fn((identity, onActivity) => {
        onActivity?.({ ...identity, stage: 'cloning-project' });
        return pull.promise;
      }),
      updateProject: vi.fn((identity, onActivity) => {
        onActivity?.({ ...identity, stage: 'validating-checkout' });
        onActivity?.({ ...identity, stage: 'fetching-and-updating' });
        return update.promise;
      }),
      reinstallProject: vi.fn((identity, onActivity) => {
        onActivity?.({ ...identity, stage: 'cloning-project' });
        onActivity?.({ ...identity, stage: 'replacing-checkout' });
        return reinstall.promise;
      }),
      removeProject: vi.fn((identity, onActivity) => {
        onActivity?.({ ...identity, stage: 'removing-checkout' });
        return remove.promise;
      }),
    };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'installed personal' });

    await user.click(screen.getByRole('button', { name: 'Not Installed' }));
    await user.click(screen.getByRole('button', { name: 'PULL' }));
    expect(screen.getByRole('button', { name: 'PULLING…' })).toBeDisabled();
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Cloning project: personal / available');
    projects = projects.map((item) => item.tool === 'available' ? { ...item, installed: true } : item);
    await act(async () => pull.resolve({ catalog: 'personal', tool: 'available' }));
    expect(await screen.findByRole('button', { name: 'Update from Remote' })).toBeEnabled();
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Project available pulled.');

    await user.click(screen.getByRole('button', { name: 'Update from Remote' }));
    expect(screen.getByRole('button', { name: 'UPDATING…' })).toBeDisabled();
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Fetching remote and updating checkout');
    await act(async () => update.reject(new Error('remote is unavailable')));
    expect(await screen.findByRole('button', { name: 'Update from Remote' })).toBeEnabled();
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Update Project failed: remote is unavailable');
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('FAILED');

    await user.click(screen.getByRole('button', { name: 'More project actions' }));
    await user.click(screen.getByRole('menuitem', { name: 'Reinstall' }));
    await user.click(screen.getByRole('button', { name: 'REINSTALL' }));
    expect(screen.getByRole('button', { name: 'REINSTALLING…' })).toBeDisabled();
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Replacing checkout: personal / available');
    await act(async () => reinstall.resolve({ catalog: 'personal', tool: 'available' }));
    await vi.waitFor(() => expect(screen.queryByRole('dialog', { name: 'Reinstall project' })).not.toBeInTheDocument());
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Project available reinstalled.');

    await user.click(screen.getByRole('button', { name: 'More project actions' }));
    await user.click(screen.getByRole('menuitem', { name: 'Remove' }));
    await user.click(screen.getByRole('button', { name: 'REMOVE CHECKOUT' }));
    expect(screen.getByRole('button', { name: 'REMOVING…' })).toBeDisabled();
    projects = projects.map((item) => item.tool === 'available' ? { ...item, installed: false } : item);
    await act(async () => remove.resolve({ catalog: 'personal', tool: 'available' }));
    expect(await screen.findByRole('button', { name: 'PULL' })).toBeEnabled();
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Project available removed.');
    expect(document.body).not.toHaveTextContent(/\d+%/);
  });

});
