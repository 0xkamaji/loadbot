import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { LoadbotMenu } from '../frontend/loadbot/LoadbotMenu';
import type {
  InteractiveSessionEventSink, LoadbotAdapter, LoadbotProject,
} from '../frontend/loadbot/contract';

const xterm = vi.hoisted(() => {
  class MockTerminal {
    static instances: MockTerminal[] = [];
    cols = 80;
    rows = 24;
    host?: HTMLElement;
    data?: (value: string) => void;
    resized?: (size: { cols: number; rows: number }) => void;
    constructor() { MockTerminal.instances.push(this); }
    loadAddon() {}
    open(host: HTMLElement) { this.host = host; }
    onData(listener: (value: string) => void) { this.data = listener; return { dispose() {} }; }
    onResize(listener: (size: { cols: number; rows: number }) => void) { this.resized = listener; return { dispose() {} }; }
    write(value: string) { this.host?.append(document.createTextNode(value)); }
    reset() { this.host?.replaceChildren(); }
    focus() {}
    dispose() {}
    emitData(value: string) { this.data?.(value); }
    emitResize(cols: number, rows: number) {
      this.cols = cols; this.rows = rows; this.resized?.({ cols, rows });
    }
  }
  return { MockTerminal };
});

vi.mock('@xterm/xterm', () => ({ Terminal: xterm.MockTerminal }));
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { fit() {} } }));

function terminalAdapter(projects: readonly LoadbotProject[]) {
  const sinks = new Map<string, InteractiveSessionEventSink>();
  let launch = 0;
  const createProjectTerminalLaunch: NonNullable<LoadbotAdapter['createProjectTerminalLaunch']> = vi.fn(async (project) => ({
    launchId: `terminal-${project.tool}-${++launch}`, label: `Project terminal — ${project.tool}`,
  }));
  const startInteractiveSession: NonNullable<LoadbotAdapter['startInteractiveSession']> = vi.fn(async (capability, sink) => {
    sinks.set(capability.launchId, sink);
    const sessionId = `session-${capability.launchId}`;
    sink({ kind: 'output', sessionId, text: 'startup-prompt> ' });
    return { sessionId, processId: `process-${capability.launchId}` };
  });
  const sendInteractiveInput: NonNullable<LoadbotAdapter['sendInteractiveInput']> = vi.fn(async () => {});
  const resizeInteractiveSession: NonNullable<LoadbotAdapter['resizeInteractiveSession']> = vi.fn(async () => {});
  const terminateInteractiveSession: NonNullable<LoadbotAdapter['terminateInteractiveSession']> = vi.fn(async () => {});
  const adapter: LoadbotAdapter = {
    readInventory: async () => projects,
    readCatalogs: async () => [{ name: 'personal', url: 'fixture', writable: true, state: 'installed', default: true }],
    openProjectFolder: vi.fn(), createProjectTerminalLaunch, startInteractiveSession,
    sendInteractiveInput, resizeInteractiveSession, terminateInteractiveSession,
    addCatalog: vi.fn(), addProject: vi.fn(), addShortcut: vi.fn(), addRecipeShortcut: vi.fn(), updateRecipeShortcut: vi.fn(),
    chooseProjectFile: vi.fn(), chooseProjectDirectory: vi.fn(), viewShortcutHelp: vi.fn(), deleteShortcuts: vi.fn(), syncCatalog: vi.fn(),
  };
  return {
    adapter, sinks, createProjectTerminalLaunch, startInteractiveSession,
    sendInteractiveInput, resizeInteractiveSession, terminateInteractiveSession,
  };
}

describe('embedded project terminal', () => {
  it('renders all bottom modes and useful empty/uninstalled states', async () => {
    const empty = terminalAdapter([]);
    const view = render(<LoadbotMenu adapter={empty.adapter} />);
    await screen.findByText('No projects in this catalog.');
    expect(screen.getByRole('tab', { name: 'COMMAND' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'ACTIVITY' })).toBeInTheDocument();
    await userEvent.click(screen.getByRole('tab', { name: 'TERMINAL' }));
    expect(screen.getByRole('tabpanel', { name: 'Terminal' })).toHaveTextContent('Select a project');
    expect(screen.getByRole('tabpanel', { name: 'Terminal' })).not.toHaveTextContent('Open Terminal to start');
    expect(empty.createProjectTerminalLaunch).not.toHaveBeenCalled();

    const unavailable = terminalAdapter([{ catalog: 'personal', tool: 'available', installed: false, entries: [] }]);
    view.rerender(<LoadbotMenu adapter={unavailable.adapter} />);
    await userEvent.click(await screen.findByRole('button', { name: 'Not Installed' }));
    await userEvent.click(screen.getByRole('tab', { name: 'TERMINAL' }));
    expect(screen.getByRole('tabpanel', { name: 'Terminal' })).toHaveTextContent('Install this project before opening a terminal.');
    expect(unavailable.createProjectTerminalLaunch).not.toHaveBeenCalled();
  });

  it('streams a bound session, routes opaque input, survives tab changes, restarts, and closes', async () => {
    xterm.MockTerminal.instances.length = 0;
    const user = userEvent.setup();
    const projects: LoadbotProject[] = [
      { catalog: 'personal', tool: 'alpha', installed: true, entries: [] },
      { catalog: 'personal', tool: 'beta', installed: true, entries: [] },
    ];
    const host = terminalAdapter(projects);
    render(<LoadbotMenu adapter={host.adapter} />);
    await screen.findByRole('button', { name: 'alpha personal' });

    await user.click(screen.getByRole('button', { name: 'OPEN TERMINAL' }));
    await screen.findByText('ACTIVE');
    expect(screen.getByRole('tab', { name: 'TERMINAL' })).toHaveAttribute('aria-selected', 'true');
    expect(host.createProjectTerminalLaunch).toHaveBeenCalledWith({ catalog: 'personal', tool: 'alpha' });
    expect(screen.getByRole('application', { name: 'Project terminal' })).toHaveTextContent('startup-prompt>');
    act(() => host.sinks.get('terminal-alpha-1')?.({
      kind: 'output', sessionId: 'session-terminal-alpha-1', text: 'shell-ready\n',
    }));
    expect(screen.getByRole('application', { name: 'Project terminal' })).toHaveTextContent('shell-ready');
    expect(screen.queryByRole('textbox', { name: 'Project terminal input' })).not.toBeInTheDocument();
    expect(screen.getByRole('tabpanel', { name: 'Terminal' }).querySelector('form')).toBeNull();

    act(() => xterm.MockTerminal.instances[0].emitData('pwd\r'));
    await waitFor(() => expect(host.sendInteractiveInput).toHaveBeenCalledWith('session-terminal-alpha-1', 'pwd\r'));
    act(() => xterm.MockTerminal.instances[0].emitData('\u0003'));
    await waitFor(() => expect(host.sendInteractiveInput).toHaveBeenCalledWith('session-terminal-alpha-1', '\u0003'));
    act(() => host.sinks.get('terminal-alpha-1')?.({
      kind: 'output', sessionId: 'session-terminal-alpha-1', text: 'pwd\r\n/project/alpha\r\nnext-prompt> ',
    }));
    expect(screen.getByRole('application', { name: 'Project terminal' })).toHaveTextContent('/project/alpha');
    act(() => xterm.MockTerminal.instances[0].emitResize(113, 37));
    expect(host.resizeInteractiveSession).toHaveBeenCalledWith('session-terminal-alpha-1', 37, 113);

    await user.click(screen.getByRole('tab', { name: 'ACTIVITY' }));
    await user.click(screen.getByRole('tab', { name: 'COMMAND' }));
    await user.click(screen.getByRole('tab', { name: 'TERMINAL' }));
    expect(host.terminateInteractiveSession).not.toHaveBeenCalled();
    expect(host.startInteractiveSession).toHaveBeenCalledTimes(1);
    expect(xterm.MockTerminal.instances).toHaveLength(1);

    await user.click(screen.getByRole('button', { name: 'beta personal' }));
    expect(screen.getByRole('tabpanel', { name: 'Terminal' })).toHaveTextContent('remains bound to personal / alpha');
    expect(host.createProjectTerminalLaunch).toHaveBeenCalledTimes(1);

    act(() => host.sinks.get('terminal-alpha-1')?.({
      kind: 'exited', sessionId: 'session-terminal-alpha-1', code: 0, cancelled: false,
    }));
    expect(screen.getByText('EXITED 0')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Restart' }));
    await screen.findByText('ACTIVE');
    expect(host.startInteractiveSession).toHaveBeenCalledTimes(2);
    await user.click(screen.getByRole('button', { name: 'Close' }));
    expect(host.terminateInteractiveSession).toHaveBeenCalledWith('session-terminal-alpha-2');
    expect(screen.getByRole('tabpanel', { name: 'Terminal' })).toHaveTextContent('Open Terminal to start');
  });
});
