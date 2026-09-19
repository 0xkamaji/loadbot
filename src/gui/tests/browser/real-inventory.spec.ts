import { expect, test } from '@playwright/test';
import inventory from '../../../../tests/fixtures/gui-inventory.json' with { type: 'json' };

const installedInventory = inventory.map((project) => ({ ...project, installed: true }));

test('normal entry uses the real read composition and preserves qualified records', async ({ page }, testInfo) => {
  // Browser transport mock only: Rust tests assert this exact JSON against real local
  // catalog/shortcut fixtures. This does not claim to launch or exercise native IPC.
  await page.addInitScript((data) => {
    Object.defineProperty(window, 'isTauri', { value: true });
    Object.defineProperty(window, '__loadbotInvocations', { value: [], writable: true });
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {
      invoke: async (command: string, args: Record<string, unknown> = {}) => {
        (window as unknown as { __loadbotInvocations: unknown[] }).__loadbotInvocations.push({ command, args });
        if (command === 'read_loadbot_inventory' && Object.keys(args).length === 0) return data;
        if (command === 'read_loadbot_catalogs' && Object.keys(args).length === 0) return [
          { name: 'alpha', url: 'test', writable: true, state: 'installed', default: true },
          { name: 'beta', url: 'test', writable: false, state: 'installed', default: false },
        ];
        if (command === 'read_loadbot_workspace_layout' && Object.keys(args).length === 0) return undefined;
        if (command === 'write_loadbot_workspace_layout' && typeof args.contents === 'string') return undefined;
        if (command === 'open_loadbot_project' && Object.keys(args).length === 2) return undefined;
        throw new Error('Unexpected command');
      },
    } });
  }, installedInventory);
  await page.goto('/desktop.html');
  await expect(page.getByRole('button', { name: 'demo alpha' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'demo beta' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'inspect [shared]' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'inspect [personal]' })).toBeVisible();
  await expect(page.getByText('LOCAL INVENTORY')).toBeVisible();
  await expect(page.getByText(/FIXTURE PREVIEW|Sample form ready/)).toHaveCount(0);
  await expect(page.getByRole('textbox')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Catalog context: alpha' })).toBeEnabled();
  await expect(page.getByRole('button', { name: 'RELOAD' })).toBeEnabled();
  await expect(page.getByRole('button', { name: 'RUN SHORTCUT' })).toHaveCount(0);
  await page.getByRole('button', { name: 'inspect [personal]' }).click();
  await expect(page.getByRole('definition').filter({ hasText: 'recipes/inspect file.py' })).toBeVisible();
  await page.getByRole('button', { name: 'RELOAD' }).click();
  await expect(page.getByRole('button', { name: 'inspect [personal]' })).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: 'Open project folder: demo (alpha)' }).click();
  expect(await page.evaluate(() => (window as unknown as { __loadbotInvocations: Array<{ command: string; args: unknown }> }).__loadbotInvocations)).toContainEqual({
    command: 'open_loadbot_project', args: { catalog: 'alpha', tool: 'demo' },
  });
  await page.getByRole('button', { name: 'demo beta' }).click();
  await expect(page.getByRole('definition').filter({ hasText: 'scripts/inspect.ps1' })).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: testInfo.outputPath('local-read-projection.png') });
});

test('native folder failures remain controlled real errors without fixture fallback', async ({ page }) => {
  await page.addInitScript((data) => {
    Object.defineProperty(window, 'isTauri', { value: true });
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {
      invoke: async (command: string) => {
        if (command === 'read_loadbot_inventory') return data;
        if (command === 'read_loadbot_catalogs') return [
          { name: 'alpha', url: 'test', writable: true, state: 'installed', default: true },
          { name: 'beta', url: 'test', writable: false, state: 'installed', default: false },
        ];
        if (command === 'read_loadbot_workspace_layout') return undefined;
        if (command === 'open_loadbot_project') throw { message: 'resolved project directory is unavailable' };
        throw new Error('Unexpected command');
      },
    } });
  }, installedInventory);
  await page.goto('/desktop.html');
  await page.getByRole('button', { name: 'Open project folder: demo (alpha)' }).click();
  await expect(page.getByText('resolved project directory is unavailable')).toBeVisible();
  await expect(page.getByText('FIXTURE PREVIEW')).toHaveCount(0);
});

for (const scenario of ['empty', 'error'] as const) {
  test(`normal entry reports ${scenario} without fictional fallback`, async ({ page }) => {
    await page.addInitScript((scenario) => {
      Object.defineProperty(window, 'isTauri', { value: true });
      Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {
        invoke: async () => {
          if (scenario === 'error') throw { message: "catalog 'offline' is not installed" };
          return [];
        },
      } });
    }, scenario);
    await page.goto('/desktop.html');
    await expect(page.getByText(scenario === 'empty'
      ? 'No projects in this catalog.'
      : "Inventory read failed: catalog 'offline' is not installed")).toBeVisible();
    await expect(page.getByRole('group', { name: 'Projects', exact: true }).getByRole('button')).toHaveCount(0);
    await expect(page.getByText('FIXTURE PREVIEW')).toHaveCount(0);
  });
}

test('ordinary browser at the native entry reports missing runtime rather than loading fixtures', async ({ page }) => {
  await page.goto('/desktop.html');
  await expect(page.getByText('Inventory read failed: Local inventory requires the native Loadbot application.')).toBeVisible();
  await expect(page.getByRole('group', { name: 'Projects', exact: true }).getByRole('button')).toHaveCount(0);
});
