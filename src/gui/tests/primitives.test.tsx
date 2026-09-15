import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';
import { ApplicationFrame, BottomDrawer, Button, Panel, PathSelector, StatusDisplay } from '../frontend/ui/components';

it('renders application-neutral controls without Loadbot data or an adapter', async () => {
  const choose = vi.fn();
  const user = userEvent.setup();
  render(<ApplicationFrame label="Example utility">
    <Panel aria-label="Settings">
      <PathSelector label="Destination" value="archive/" readOnly action={<Button onClick={choose}>Select destination</Button>} />
      <StatusDisplay>Choose a destination</StatusDisplay>
    </Panel>
    <BottomDrawer open id="information" label="Information"><p>Parent-provided content</p></BottomDrawer>
  </ApplicationFrame>);
  expect(screen.getByRole('region', { name: 'Example utility' })).toBeInTheDocument();
  expect(screen.getByLabelText('Destination')).toHaveValue('archive/');
  const drawer = screen.getByRole('region', { name: 'Information' });
  expect(screen.getByText('Parent-provided content')).toBeVisible();
  expect((drawer.closest('.lb-theme') as HTMLElement).style.getPropertyValue('--lb-terminal-surface')).toBe('#ecd4af');
  expect(screen.queryByText(/fixture|terminal|project|shortcut/i)).not.toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: 'Select destination' }));
  expect(choose).toHaveBeenCalledOnce();
});
