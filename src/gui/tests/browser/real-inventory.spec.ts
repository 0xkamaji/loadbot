import { expect, test } from '@playwright/test';
import inventory from '../../../../tests/fixtures/gui-inventory.json' with { type: 'json' };

test('normal entry uses the real read composition and preserves qualified records', async ({ page }, testInfo) => {
  // Browser transport mock only: Rust tests assert this exact JSON against real local
  // catalog/shortcut fixtures. This does not claim to launch or exercise native IPC.
  await page.addInitScript((data) => {
    Object.defineProperty(window, 'isTauri', { value: true });
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {
      invoke: async (command: string, args: Record<string, unknown>) => {
        if (command !== 'read_loadbot_inventory' || Object.keys(args).length !== 0) throw new Error('Unexpected command');
        return data;
      },
    } });
  }, inventory);
  await page.goto('/desktop.html');
  await expect(page.getByRole('button', { name: 'demo alpha' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'demo beta' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'inspect [shared]' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'inspect [personal]' })).toBeVisible();
  await expect(page.getByText('LOCAL INVENTORY')).toBeVisible();
  await expect(page.getByText(/FIXTURE PREVIEW|Sample form ready/)).toHaveCount(0);
  await expect(page.getByRole('textbox')).toHaveCount(0);
  for (const name of ['+ Add project', 'Refresh catalog', 'RUN SHORTCUT', 'Open project folder']) {
    await expect(page.getByRole('button', { name, exact: true })).toBeDisabled();
  }
  await page.getByRole('button', { name: 'inspect [personal]' }).click();
  await expect(page.getByText('Personal · Runner not specified · recipes/inspect file.py')).toBeVisible();
  await page.getByRole('button', { name: 'demo beta' }).click();
  await expect(page.getByText('Shared · powershell · scripts/inspect.ps1')).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: testInfo.outputPath('local-read-projection.png') });
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
      ? 'No projects with commands or shortcuts in local Loadbot data.'
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
