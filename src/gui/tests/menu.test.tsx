import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { fixtureMenuDependencies } from '../frontend/hosts/fixtureComposition';
import type { LoadbotAdapter, LoadbotProject, ShortcutIdentity } from '../frontend/loadbot/contract';
import { LoadbotMenu } from '../frontend/loadbot/LoadbotMenu';

const projectRows = () => within(screen.getByRole('group', { name: 'Projects' }));
const shortcutRows = () => within(screen.getByRole('group', { name: 'Shortcuts' }));
const deferred = <T,>() => {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
};

describe('injected menu outside Tauri', () => {
  const adapter = (projects: readonly LoadbotProject[], open = vi.fn(async () => {})): LoadbotAdapter => ({
    readInventory: async () => projects,
    readCatalogs: async () => [...new Set(projects.map((item) => item.catalog))].map((name, index) => ({ name, url: 'fixture', writable: true, state: 'installed' as const, default: index === 0 })),
    openProjectFolder: open, addCatalog: vi.fn(), addProject: vi.fn(), addShortcut: vi.fn(), addRecipeShortcut: vi.fn(), updateRecipeShortcut: vi.fn(),
    chooseProjectFile: vi.fn(), chooseProjectDirectory: vi.fn(), viewShortcutHelp: vi.fn(), deleteShortcuts: vi.fn(), syncCatalog: vi.fn(),
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
    expect(screen.getByRole('tab', { name: 'COMMAND' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'ACTIVITY' })).toBeInTheDocument();
    expect(screen.queryByRole('tab', { name: 'TERMINAL' })).not.toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Loadbot command' })).toBeInTheDocument();
    const consoleButton = screen.getByRole('button', { name: 'Console' });
    await user.click(consoleButton);
    expect(screen.queryByRole('region', { name: 'Bottom workspace' })).not.toBeInTheDocument();
    expect(consoleButton).toHaveAttribute('aria-expanded', 'false');
    await user.click(consoleButton);
    expect(screen.getByRole('region', { name: 'Bottom workspace' })).toBeVisible();
    expect(consoleButton).toHaveAttribute('aria-expanded', 'true');
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
    expect(screen.getByRole('tabpanel', { name: 'Command' })).toHaveTextContent('shortcuts [project]');
    expect(screen.getByRole('tabpanel', { name: 'Command' })).toHaveTextContent('inspect <project> [shortcut]');
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

  it('inspects isolated Legacy, Run Recipe, and Launch Recipe fixtures without enabling management', async () => {
    const user = userEvent.setup();
    render(<LoadbotMenu {...fixtureMenuDependencies} />);
    await user.click(await projectRows().findByRole('button', { name: 'rotbot personal' }));
    await user.click(shortcutRows().getByRole('button', { name: 'Build report' }));
    expect(screen.getByText('Run Recipe')).toBeInTheDocument();
    expect(screen.getByText('scripts/report.py')).toBeInTheDocument();
    expect(screen.getByText('python')).toBeInTheDocument();
    await user.click(shortcutRows().getByRole('button', { name: 'Open dashboard' }));
    expect(screen.getByText('Launch Application')).toBeInTheDocument();
    expect(screen.getByText('Shared catalog shortcuts are read-only here.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'EDIT' })).not.toBeInTheDocument();
    await user.click(shortcutRows().getByRole('button', { name: 'Inspect workspace' }));
    expect(screen.getByText(/scripts\/inspect.py/)).toBeInTheDocument();
  });

  it('completes registered commands and semantic identities without submitting or creating activity', async () => {
    const user = userEvent.setup();
    render(<LoadbotMenu {...fixtureMenuDependencies} />);
    await screen.findByRole('button', { name: 're-toolkit personal' });
    const input = screen.getByRole('combobox', { name: 'Loadbot command' }) as HTMLInputElement;

    await user.type(input, 'sho{Tab}');
    expect(input).toHaveValue('shortcuts');
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
    const addRecipeShortcut = vi.fn(async (input) => {
      projects = projects.map((project) => project.tool === input.tool ? { ...project, entries: [{ name: input.name, recipe: input.recipe, source: 'personal' as const }] } : project);
      return { catalog: input.catalog, tool: input.tool, name: input.name };
    });
    const managed: LoadbotAdapter = {
      ...adapter(projects), readInventory: async () => projects, readCatalogs: async () => catalogs,
      addProject, addRecipeShortcut,
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
    expect(screen.getByRole('heading', { name: 'CREATE SHORTCUT' })).toBeInTheDocument();
    expect(screen.queryByText(/Legacy|Launch Application|Run Recipe/)).not.toBeInTheDocument();
    await user.type(screen.getByLabelText('Name *'), 'inspect');
    await user.type(screen.getByLabelText('Target *'), 'scripts/inspect.py');
    expect(screen.getByLabelText('Run with')).toHaveValue('python');
    await user.click(screen.getByRole('button', { name: 'CREATE SHORTCUT' }));
    expect(await shortcutRows().findByRole('button', { name: 'inspect' })).toHaveAttribute('aria-pressed', 'true');
    expect(addRecipeShortcut).toHaveBeenCalledWith(expect.objectContaining({
      catalog: 'personal', tool: 'new-project', recipe: expect.objectContaining({ behavior: 'run', program: { type: 'interpreter', runner: 'python' } }),
    }));

    await user.click(screen.getByRole('button', { name: 'Catalog context: personal' }));
    await user.click(screen.getByRole('button', { name: 'REFRESH CATALOG' }));
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
    expect(await screen.findByRole('button', { name: 'UPDATE' })).toBeEnabled();
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Project available pulled.');

    await user.click(screen.getByRole('button', { name: 'UPDATE' }));
    expect(screen.getByRole('button', { name: 'UPDATING…' })).toBeDisabled();
    expect(screen.getByRole('tabpanel', { name: 'Activity' })).toHaveTextContent('Fetching remote and updating checkout');
    await act(async () => update.reject(new Error('remote is unavailable')));
    expect(await screen.findByRole('button', { name: 'UPDATE' })).toBeEnabled();
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

  it('creates, inspects, reopens, and edits a structured Recipe without execution', async () => {
    const user = userEvent.setup();
    let projects: readonly LoadbotProject[] = [{ catalog: 'personal', tool: 'demo', entries: [] }];
    const addRecipeShortcut = vi.fn(async (input) => {
      projects = [{ ...projects[0]!, entries: [{ name: input.name, description: input.description, recipe: input.recipe, source: 'personal' as const }] }];
      return { catalog: input.catalog, tool: input.tool, name: input.name };
    });
    const updateRecipeShortcut = vi.fn(async (input) => {
      projects = [{ ...projects[0]!, entries: [{ name: input.name, description: input.description, recipe: input.recipe, source: 'personal' as const }] }];
      return { catalog: input.catalog, tool: input.tool, name: input.name };
    });
    const managed: LoadbotAdapter = {
      ...adapter(projects), readInventory: async () => projects, addRecipeShortcut, updateRecipeShortcut,
    };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'demo personal' });

    await user.click(screen.getByRole('button', { name: '+ ADD SHORTCUT' }));
    expect(screen.getByRole('dialog', { name: 'Create shortcut' })).toBeInTheDocument();
    expect(screen.queryByText('LAUNCH APPLICATION')).not.toBeInTheDocument();
    expect(screen.queryByText(/LEGACY/)).not.toBeInTheDocument();
    await user.type(screen.getByLabelText('Name *'), 'build');
    await user.type(screen.getByLabelText('Description (optional)'), 'Build project');
    await user.type(screen.getByLabelText('Target *'), 'scripts/build.py');
    expect(screen.getByLabelText('Run with')).toHaveValue('python');
    await user.selectOptions(screen.getByLabelText('Run with'), 'bash');
    await user.click(screen.getByRole('button', { name: '+ ADD PARAMETER' }));
    const parameterName = screen.getAllByLabelText('Name *')[1]!;
    await user.clear(parameterName);
    await user.type(parameterName, 'Profile');
    expect(screen.queryByLabelText('Parameter ID *')).not.toBeInTheDocument();
    await user.click(screen.getAllByRole('button', { name: 'ADVANCED' })[0]!);
    expect(screen.getByLabelText('Parameter ID *')).toHaveValue('profile');
    await user.clear(screen.getByLabelText('Parameter ID *'));
    await user.type(screen.getByLabelText('Parameter ID *'), 'build-profile');
    await user.clear(parameterName);
    await user.type(parameterName, 'Mode');
    expect(screen.getByLabelText('Parameter ID *')).toHaveValue('build-profile');
    expect(screen.getByRole('region', { name: 'Shortcut preview' })).toHaveTextContent('bash scripts/build.py {Mode}');
    await user.click(screen.getByRole('button', { name: 'CREATE SHORTCUT' }));

    expect(addRecipeShortcut).toHaveBeenCalledWith(expect.objectContaining({
      name: 'build', recipe: expect.objectContaining({
        behavior: 'run', program: { type: 'interpreter', runner: 'bash' },
        arguments: [{ type: 'project-path', path: 'scripts/build.py' }, expect.objectContaining({ id: 'build-profile' })],
      }),
    }));
    expect(await screen.findByRole('heading', { name: 'build' })).toBeInTheDocument();
    expect(screen.getByText('Run Recipe')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'EDIT' }));
    expect(screen.getByRole('dialog', { name: 'Edit shortcut' })).toBeInTheDocument();
    expect(screen.getAllByLabelText('Name *')[0]).toBeDisabled();
    expect(screen.getByLabelText('Target *')).toHaveValue('scripts/build.py');
    await user.click(screen.getByRole('button', { name: 'SAVE SHORTCUT' }));
    expect(updateRecipeShortcut).toHaveBeenCalledWith(expect.objectContaining({ recipe: expect.objectContaining({ behavior: 'run' }) }));
    expect(await screen.findByText('Run Recipe')).toBeInTheDocument();
    expect(screen.queryByText(/output/i)).not.toBeInTheDocument();
  });

  it('shows dismissible raw target help and clears it when the target changes', async () => {
    const user = userEvent.setup();
    const viewShortcutHelp = vi.fn(async () => ({
      commandAttempted: ['python', 'scripts/tool.py', '--help'], stdout: 'Usage: tool [options]\n  --verbose\n',
      stderr: 'Additional help from stderr\n', exitStatus: 2, detectedHelpFlag: '--help' as const,
    }));
    const managed = { ...adapter([{ catalog: 'personal', tool: 'demo', entries: [] }]), viewShortcutHelp };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'demo personal' });
    await user.click(screen.getByRole('button', { name: '+ ADD SHORTCUT' }));
    await user.type(screen.getByLabelText('Target *'), 'scripts/tool.py');
    const preview = screen.getByRole('region', { name: 'Shortcut preview' }).textContent;

    await user.click(screen.getByRole('button', { name: 'VIEW HELP' }));
    const help = await screen.findByRole('region', { name: 'Target help' });
    expect(help).toHaveTextContent('Usage: tool [options]');
    expect(help).toHaveTextContent('Additional help from stderr');
    expect(help).toHaveTextContent('--help · exit 2');
    expect(screen.getByRole('region', { name: 'Shortcut preview' }).textContent).toBe(preview);
    expect(viewShortcutHelp).toHaveBeenCalledWith(expect.objectContaining({ target: 'scripts/tool.py', runner: 'python' }));

    await user.clear(screen.getByLabelText('Target *'));
    await user.type(screen.getByLabelText('Target *'), 'scripts/other.py');
    expect(screen.queryByRole('region', { name: 'Target help' })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'VIEW HELP' }));
    await screen.findByRole('region', { name: 'Target help' });
    await user.click(screen.getByRole('button', { name: 'HIDE' }));
    expect(screen.queryByRole('region', { name: 'Target help' })).not.toBeInTheDocument();
  });

  it('browses portable author-time paths and manages only personal shortcuts with confirmation', async () => {
    const user = userEvent.setup();
    let projects: readonly LoadbotProject[] = [{ catalog: 'personal', tool: 'demo', entries: [
      { name: 'Personal one', path: 'one.sh', source: 'personal' },
      { name: 'Shared command', path: 'shared.sh', source: 'catalog' },
      { name: 'Personal two', path: 'two.sh', source: 'personal' },
    ] }];
    const chooseProjectFile = vi.fn()
      .mockResolvedValueOnce('scripts/tool.py')
      .mockResolvedValueOnce(undefined);
    const chooseProjectDirectory = vi.fn(async () => 'scripts/tools');
    const deleteShortcuts = vi.fn(async (identities: readonly ShortcutIdentity[]) => {
      const names = new Set(identities.map((item) => item.name));
      projects = [{ ...projects[0]!, entries: projects[0]!.entries.filter((item) => !names.has(item.name)) }];
      return identities.length;
    });
    const managed: LoadbotAdapter = {
      ...adapter(projects), readInventory: async () => projects, chooseProjectFile, chooseProjectDirectory, deleteShortcuts,
    };
    render(<LoadbotMenu adapter={managed} />);
    await projectRows().findByRole('button', { name: 'demo personal' });

    await user.click(screen.getByRole('button', { name: '+ ADD SHORTCUT' }));
    await user.click(screen.getAllByRole('button', { name: 'BROWSE' })[0]!);
    expect(screen.getByLabelText('Target *')).toHaveValue('scripts/tool.py');
    expect(screen.getByLabelText('Run with')).toHaveValue('python');
    await user.click(screen.getByRole('button', { name: 'ADVANCED' }));
    await user.selectOptions(screen.getByLabelText('Location'), 'project-relative');
    await user.click(screen.getAllByRole('button', { name: 'BROWSE' })[1]!);
    expect(screen.getByLabelText('Folder *')).toHaveValue('scripts/tools');
    await user.click(screen.getAllByRole('button', { name: 'BROWSE' })[0]!);
    expect(screen.getByLabelText('Target *')).toHaveValue('scripts/tool.py');
    expect(screen.queryByText(/Could not choose/)).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'CANCEL' }));

    expect(screen.getByRole('button', { name: 'DELETE' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'DELETE' }));
    expect(screen.getByRole('dialog', { name: 'Delete shortcut' })).toHaveTextContent('does not delete the tool or any files');
    await user.click(screen.getByRole('button', { name: 'CANCEL' }));
    expect(deleteShortcuts).not.toHaveBeenCalled();
    await user.click(shortcutRows().getByRole('button', { name: 'Shared command' }));
    expect(screen.queryByRole('button', { name: 'DELETE' })).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'MANAGE' }));
    expect(screen.getByRole('checkbox', { name: /Shared command/ })).toBeDisabled();
    await user.click(screen.getByRole('checkbox', { name: /Personal one/ }));
    await user.click(screen.getByRole('button', { name: 'DONE' }));
    expect(screen.queryByRole('checkbox', { name: /Personal one/ })).not.toBeInTheDocument();
    expect(deleteShortcuts).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'MANAGE' }));
    await user.click(screen.getByRole('checkbox', { name: /Personal one/ }));
    await user.click(screen.getByRole('checkbox', { name: /Personal two/ }));
    expect(screen.getByText('2 selected')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'DELETE SELECTED' }));
    expect(screen.getByRole('dialog', { name: 'Delete shortcuts' })).toHaveTextContent('Personal one');
    await user.click(screen.getByRole('button', { name: 'DELETE 2' }));
    await vi.waitFor(() => expect(deleteShortcuts).toHaveBeenCalledOnce());
    expect(deleteShortcuts.mock.calls[0]?.[0]).toHaveLength(2);
    expect(await shortcutRows().findByRole('button', { name: 'Shared command' })).toBeInTheDocument();
    expect(shortcutRows().queryByRole('button', { name: 'Personal one' })).not.toBeInTheDocument();
  });
});
