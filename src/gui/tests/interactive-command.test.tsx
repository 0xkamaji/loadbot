import { act, fireEvent, render, screen } from '@testing-library/react';
import { useSyncExternalStore } from 'react';
import { describe, expect, it, vi } from 'vitest';

import { createLoadbotApplication } from '../frontend/loadbot/application/controller';
import { fixtureAdapter } from '../frontend/loadbot/fixtures/adapter';
import { CommandPane } from '../frontend/loadbot/view/CommandPane';
import type { InteractiveSessionEventSink, LoadbotAdapter } from '../frontend/loadbot/contract';

describe('interactive COMMAND presentation', () => {
  it('shows session output, routes ephemeral input, cancels, and returns to structured commands', async () => {
    let onEvent: InteractiveSessionEventSink = () => {};
    const sendInteractiveInput: NonNullable<LoadbotAdapter['sendInteractiveInput']> = vi.fn(async () => {});
    const terminateInteractiveSession: NonNullable<LoadbotAdapter['terminateInteractiveSession']> = vi.fn(async () => {});
    const adapter: LoadbotAdapter = {
      ...fixtureAdapter,
      startInteractiveSession: vi.fn(async (_launch, sink) => {
        onEvent = sink;
        return { sessionId: 'session-ui', processId: 'process-ui' };
      }),
      sendInteractiveInput,
      terminateInteractiveSession,
    };
    const application = createLoadbotApplication(adapter);
    function Harness() {
      const state = useSyncExternalStore(application.subscribe, application.getSnapshot);
      return <CommandPane state={state} actions={application.actions} />;
    }
    render(<Harness />);
    const stop = application.start();
    await vi.waitFor(() => expect(application.getSnapshot().inventory.status).toBe('ready'));

    await act(() => application.actions.startInteractiveSession({ launchId: 'opaque-ui', label: 'Authentication prompt' }));
    expect(screen.getByRole('status')).toHaveTextContent('Interactive session active');
    act(() => onEvent({ kind: 'output', sessionId: 'session-ui', text: 'Enter value: ' }));
    expect(screen.getByLabelText('Interactive session output')).toHaveTextContent('Enter value:');

    const input = screen.getByRole('combobox', { name: 'Interactive session input' });
    fireEvent.change(input, { target: { value: 'ephemeral input' } });
    fireEvent.submit(input.closest('form')!);
    await vi.waitFor(() => expect(sendInteractiveInput).toHaveBeenCalledWith('session-ui', 'ephemeral input\r'));
    expect(application.getSnapshot().command.history).toEqual([]);
    expect(screen.queryByText('ephemeral input')).not.toBeInTheDocument();

    fireEvent.change(input, { target: { value: 'unsent secret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Terminate session' }));
    await vi.waitFor(() => expect(terminateInteractiveSession).toHaveBeenCalledWith('session-ui'));
    act(() => onEvent({ kind: 'exited', sessionId: 'session-ui', code: 1, cancelled: true }));
    expect(screen.getByRole('combobox', { name: 'Loadbot command' })).toHaveValue('');
    expect(screen.getByLabelText('Interactive session output')).toHaveTextContent('Interactive session cancelled.');

    const command = screen.getByRole('combobox', { name: 'Loadbot command' });
    fireEvent.change(command, { target: { value: 'projects' } });
    fireEvent.submit(command.closest('form')!);
    expect(screen.getByText(/^Projects/)).toBeInTheDocument();
    stop();
  });
});
