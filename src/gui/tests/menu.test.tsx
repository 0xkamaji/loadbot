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
