import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { fixtureMenuDependencies } from '../frontend/hosts/fixtureComposition';
import type { LoadbotAdapter, LoadbotProject } from '../frontend/loadbot/contract';
import { LoadbotMenu } from '../frontend/loadbot/LoadbotMenu';

const projectRows = () => within(screen.getByRole('group', { name: 'Projects' }));
const shortcutRows = () => within(screen.getByRole('group', { name: 'Shortcuts' }));

describe('injected menu outside Tauri', () => {
  it('changes project/shortcut, resets isolated forms, and preserves state through the drawer', async () => {
    const user = userEvent.setup();
    render(<LoadbotMenu {...fixtureMenuDependencies} />);
    await screen.findByRole('heading', { name: 'Malware triage' });
    expect(screen.getByRole('status')).toHaveTextContent('Input required: input folder');
    await user.click(screen.getByRole('button', { name: 'Use sample input folder' }));
    await user.click(screen.getByRole('checkbox'));
    expect(screen.getByRole('status')).toHaveTextContent('Sample form ready');
    expect(screen.getByRole('button', { name: 'RUN SHORTCUT' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Terminal' }));
    expect(screen.getByRole('region', { name: 'Terminal placeholder' })).toBeVisible();
    expect(screen.getByRole('region', { name: 'Terminal placeholder' })).not.toContainElement(screen.getByLabelText('Input folder *'));
    await user.click(screen.getByRole('button', { name: 'Terminal' }));
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
    await user.keyboard('{End}{Enter}');
    const shared = shortcutRows().getByRole('button', { name: 'Export strings [shared]' });
    shared.focus();
    await user.keyboard('{ArrowDown} ');
    expect(screen.getByText('Personal variant; this remains independently selectable.')).toBeInTheDocument();
  });

  it('renders a supplied adapter in an ordinary parent and delegates close to that parent', async () => {
    const adapter: LoadbotAdapter = { readInventory: vi.fn(async () => [{ catalog: 'test', tool: 'injected', entries: [] }]) };
    const close = vi.fn();
    const user = userEvent.setup();
    render(<div style={{ width: 700, height: 500 }}><LoadbotMenu adapter={adapter} host={{ onClose: close }} /></div>);
    expect(await screen.findByRole('button', { name: 'injected test' })).toBeInTheDocument();
    expect(screen.getByText('No shortcuts in this fixture project.')).toBeInTheDocument();
    await user.keyboard('{Escape}');
    expect(close).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Close Loadbot menu' }));
    expect(close).toHaveBeenCalledOnce();
    expect(adapter.readInventory).toHaveBeenCalledOnce();
  });

  it('ignores stale adapter responses and displays empty/error results honestly', async () => {
    let resolve!: (projects: readonly LoadbotProject[]) => void;
    const slow: LoadbotAdapter = { readInventory: () => new Promise((done) => { resolve = done; }) };
    const empty: LoadbotAdapter = { readInventory: async () => [] };
    const view = render(<LoadbotMenu adapter={slow} />);
    await act(async () => {});
    view.rerender(<LoadbotMenu adapter={empty} />);
    await screen.findByText('No fixture projects available.');
    await act(async () => resolve([{ catalog: 'old', tool: 'stale', entries: [] }]));
    expect(screen.queryByRole('button', { name: 'stale old' })).not.toBeInTheDocument();
    view.rerender(<LoadbotMenu adapter={{ readInventory: async () => { throw new Error('Fixture read failed'); } }} />);
    expect(await screen.findByText('Fixture unavailable: Fixture read failed')).toBeInTheDocument();
  });
});
