import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { LoadbotMenu } from '../frontend/loadbot/LoadbotMenu';
import { fixtureAdapter } from '../frontend/loadbot/fixtures/adapter';
import { clampSplit, Splitter } from '../frontend/ui/Splitter';
import { decodePaneSizes, defaultPaneSizes, encodePaneSizes, type WorkspaceLayoutStore } from '../frontend/loadbot/view/workspaceLayout';

describe('workspace pane dimensions', () => {
  it('uses defaults for missing, malformed, old, and unavailable persisted state', () => {
    expect(decodePaneSizes(undefined)).toEqual(defaultPaneSizes);
    for (const value of ['bad json', '{"version":0,"projects":200}', '{"version":1,"projects":"wide","shortcuts":200,"terminal":100}']) {
      expect(decodePaneSizes(value)).toEqual(defaultPaneSizes);
    }
  });

  it('restores clamped values and persists only versioned presentation state', () => {
    const value = decodePaneSizes(JSON.stringify({ version: 1, projects: 9999, shortcuts: -10, terminal: 160 }));
    expect(value).toEqual({ projects: 560, shortcuts: 110, terminal: 160 });
    expect(encodePaneSizes(value)).toBe(JSON.stringify({ version: 1, ...value }));
  });

  it('loads once, saves all three preferences independently, and never saves measurement clamps', async () => {
    const write = vi.fn<WorkspaceLayoutStore['write']>(async () => {});
    const store: WorkspaceLayoutStore = {
      read: vi.fn(async () => encodePaneSizes({ projects: 320, shortcuts: 240, terminal: 180 })),
      write,
    };
    render(<LoadbotMenu adapter={fixtureAdapter} mode="fixture" workspaceLayoutStore={store} />);
    const projects = await screen.findByRole('separator', { name: 'Resize projects pane' });
    const shortcuts = screen.getByRole('separator', { name: 'Resize shortcuts and selected shortcut' });
    const terminal = screen.getByRole('separator', { name: 'Resize terminal pane' });
    await vi.waitFor(() => expect(projects).toHaveAttribute('aria-valuenow', '320'));
    expect(shortcuts).toHaveAttribute('aria-valuenow', '240');
    expect(terminal).toHaveAttribute('aria-valuenow', '180');
    fireEvent(window, new Event('resize'));
    expect(write).not.toHaveBeenCalled();

    const pointer = (type: string, clientX: number) => {
      const event = new Event(type, { bubbles: true, cancelable: true });
      Object.defineProperties(event, {
        pointerId: { value: 9 }, clientX: { value: clientX }, clientY: { value: 0 }, button: { value: 0 },
      });
      fireEvent(projects, event);
    };
    pointer('pointerdown', 320);
    pointer('pointermove', 340);
    expect(write).not.toHaveBeenCalled();
    pointer('pointerup', 340);
    await vi.waitFor(() => expect(write).toHaveBeenCalledTimes(1));
    expect(JSON.parse(write.mock.calls[0][0])).toEqual({ version: 1, projects: 340, shortcuts: 240, terminal: 180 });
    fireEvent.keyDown(projects, { key: 'ArrowRight' });
    expect(JSON.parse(write.mock.calls[1][0])).toEqual({ version: 1, projects: 352, shortcuts: 240, terminal: 180 });
    fireEvent.keyDown(shortcuts, { key: 'ArrowDown' });
    expect(JSON.parse(write.mock.calls[2][0])).toEqual({ version: 1, projects: 352, shortcuts: 252, terminal: 180 });
    fireEvent.keyDown(terminal, { key: 'ArrowUp' });
    expect(JSON.parse(write.mock.calls[3][0])).toEqual({ version: 1, projects: 352, shortcuts: 252, terminal: 192 });
    fireEvent.doubleClick(projects);
    expect(JSON.parse(write.mock.calls[4][0])).toEqual({ version: 1, projects: 260, shortcuts: 252, terminal: 192 });
  });

  it('clamps restored preferences to the current layout without replacing the stored values', async () => {
    const write = vi.fn<WorkspaceLayoutStore['write']>(async () => {});
    const store: WorkspaceLayoutStore = {
      read: async () => encodePaneSizes({ projects: 560, shortcuts: 640, terminal: 440 }),
      write,
    };
    render(<LoadbotMenu adapter={fixtureAdapter} mode="fixture" workspaceLayoutStore={store} />);
    await vi.waitFor(() => expect(screen.getByRole('separator', { name: 'Resize projects pane' })).toHaveAttribute('aria-valuenow', '532'));
    expect(screen.getByRole('separator', { name: 'Resize shortcuts and selected shortcut' })).toHaveAttribute('aria-valuenow', '292');
    expect(screen.getByRole('separator', { name: 'Resize terminal pane' })).toHaveAttribute('aria-valuenow', '322');
    expect(write).not.toHaveBeenCalled();
  });

  it('falls back when native preference I/O is unavailable and never crashes on save failure', async () => {
    const write = vi.fn<WorkspaceLayoutStore['write']>(async () => { throw new Error('read-only filesystem'); });
    const store: WorkspaceLayoutStore = { read: async () => { throw new Error('unavailable'); }, write };
    render(<LoadbotMenu adapter={fixtureAdapter} mode="fixture" workspaceLayoutStore={store} />);
    const projects = await screen.findByRole('separator', { name: 'Resize projects pane' });
    expect(projects).toHaveAttribute('aria-valuenow', String(defaultPaneSizes.projects));
    fireEvent.keyDown(projects, { key: 'ArrowRight' });
    await vi.waitFor(() => expect(write).toHaveBeenCalledOnce());
    expect(projects).toHaveAttribute('aria-valuenow', '272');
  });

  it('drags, clamps, supports the keyboard, and resets a focused splitter', () => {
    const change = vi.fn();
    const commit = vi.fn();
    const reset = vi.fn();
    const view = render(<Splitter orientation="vertical" label="Resize test pane" value={200}
      limits={() => ({ min: 120, max: 280 })} onChange={change} onCommit={commit} onReset={reset} />);
    const splitter = screen.getByRole('separator', { name: 'Resize test pane' });
    const pointer = (type: string, clientX: number, button?: number) => {
      const event = new Event(type, { bubbles: true, cancelable: true });
      Object.defineProperties(event, {
        pointerId: { value: 7 }, clientX: { value: clientX }, clientY: { value: 0 }, button: { value: button ?? 0 },
      });
      fireEvent(splitter, event);
    };
    pointer('pointerdown', 200, 0);
    pointer('pointermove', 500);
    pointer('pointerup', 500);
    expect(change).toHaveBeenLastCalledWith(280);
    expect(commit).toHaveBeenLastCalledWith(280);
    fireEvent.keyDown(splitter, { key: 'ArrowLeft' });
    expect(change).toHaveBeenLastCalledWith(188);
    fireEvent.doubleClick(splitter);
    expect(reset).toHaveBeenCalledOnce();
    expect(clampSplit(100, { min: 150, max: 120 })).toBe(150);
    view.unmount();
  });
});
