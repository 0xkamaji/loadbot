import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { clampSplit, Splitter } from '../frontend/ui/Splitter';
import { defaultPaneSizes, paneStorageKey, persistPaneSizes, restorePaneSizes } from '../frontend/loadbot/view/workspaceLayout';

describe('workspace pane dimensions', () => {
  it('uses defaults for missing, malformed, old, and unavailable persisted state', () => {
    expect(restorePaneSizes(undefined)).toEqual(defaultPaneSizes);
    for (const value of ['bad json', '{"version":0,"projects":200}', '{"version":1,"projects":"wide","shortcuts":200,"terminal":100}']) {
      expect(restorePaneSizes({ getItem: () => value })).toEqual(defaultPaneSizes);
    }
    expect(restorePaneSizes({ getItem: () => { throw new Error('storage unavailable'); } })).toEqual(defaultPaneSizes);
  });

  it('restores clamped values and persists only versioned presentation state', () => {
    const value = restorePaneSizes({ getItem: () => JSON.stringify({ version: 1, projects: 9999, shortcuts: -10, terminal: 160 }) });
    expect(value).toEqual({ projects: 560, shortcuts: 110, terminal: 160 });
    const setItem = vi.fn();
    persistPaneSizes(value, { setItem });
    expect(setItem).toHaveBeenCalledWith(paneStorageKey, JSON.stringify({ version: 1, ...value }));
    expect(() => persistPaneSizes(value, { setItem: () => { throw new Error('blocked'); } })).not.toThrow();
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
