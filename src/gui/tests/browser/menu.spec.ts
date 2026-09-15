import { expect, test } from '@playwright/test';

function contrastRatio(foreground: string, background: string): number {
  const luminance = (color: string) => {
    const source = color.trim();
    const values = source.startsWith('#')
      ? [source.slice(1, 3), source.slice(3, 5), source.slice(5, 7)].map((value) => Number.parseInt(value, 16))
      : source.match(/[\d.]+/g)!.slice(0, 3).map(Number);
    const channels = values.map((value) => {
      const channel = value / 255;
      return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
    });
    return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
  };
  const [lighter, darker] = [luminance(foreground), luminance(background)].sort((a, b) => b - a);
  return (lighter + 0.05) / (darker + 0.05);
}

test('console terminology and light-surface activity contrast remain clear', async ({ page }) => {
  await page.goto('/fixture.html');
  const consoleButton = page.getByRole('button', { name: 'Console', exact: true });
  const commandTab = page.getByRole('tab', { name: 'COMMAND' });
  const activityTab = page.getByRole('tab', { name: 'ACTIVITY' });
  await expect(consoleButton).toBeVisible();
  await expect(commandTab).toHaveAttribute('aria-selected', 'true');
  await expect(activityTab).toHaveAttribute('aria-selected', 'false');
  await expect(page.getByRole('combobox', { name: 'Loadbot command' })).toBeVisible();
  await expect(page.getByRole('tab', { name: 'TERMINAL' })).toHaveCount(0);

  const colors = await commandTab.evaluate((element) => {
    const theme = getComputedStyle(element.closest('.lb-theme')!);
    return {
      surface: theme.getPropertyValue('--lb-terminal-surface'),
      primary: theme.getPropertyValue('--lb-terminal-text'),
      muted: theme.getPropertyValue('--lb-terminal-muted'),
      success: theme.getPropertyValue('--lb-terminal-success'),
      error: theme.getPropertyValue('--lb-terminal-error'),
      activeTab: getComputedStyle(element).color,
      inactiveTab: getComputedStyle(element.nextElementSibling!).color,
    };
  });
  for (const foreground of [colors.primary, colors.muted, colors.success, colors.error]) {
    expect(contrastRatio(foreground, colors.surface)).toBeGreaterThanOrEqual(4.5);
  }
  expect(contrastRatio(colors.activeTab, colors.surface)).toBeGreaterThanOrEqual(4.5);
  expect(contrastRatio(colors.inactiveTab, colors.surface)).toBeGreaterThanOrEqual(4.5);
  expect(colors.activeTab).not.toBe(colors.inactiveTab);

  await activityTab.click();
  await expect(activityTab).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('tabpanel', { name: 'Activity' })).toBeVisible();
  await consoleButton.click();
  await expect(page.getByRole('region', { name: 'Bottom workspace' })).toBeHidden();
  await consoleButton.click();
  await expect(page.getByRole('tabpanel', { name: 'Activity' })).toBeVisible();
});

test('console keeps a flat interior and decorative frame across sizes', async ({ page }, testInfo) => {
  await page.goto('/fixture.html');
  const drawer = page.getByRole('region', { name: 'Bottom workspace' });
  const splitter = page.getByRole('separator', { name: 'Resize console pane' });
  const assertSeparatedFrame = async () => {
    const style = await drawer.evaluate((element) => {
      const computed = getComputedStyle(element);
      return {
        background: computed.backgroundColor,
        backgroundClip: computed.backgroundClip,
        borderImageSlice: computed.borderImageSlice,
        borderImageSource: computed.borderImageSource,
      };
    });
    expect(style.background).toBe('rgb(236, 212, 175)');
    expect(style.backgroundClip).toBe('padding-box');
    expect(style.borderImageSlice).not.toContain('fill');
    expect(style.borderImageSource).toContain('panel');
  };

  await assertSeparatedFrame();
  const boundary = (await splitter.boundingBox())!;
  await page.mouse.move(boundary.x + 10, boundary.y + 4);
  await page.mouse.down();
  await page.mouse.move(boundary.x + 10, boundary.y - 110);
  await page.mouse.up();
  await expect(splitter).toHaveAttribute('aria-valuenow', '250');
  await assertSeparatedFrame();
  await page.screenshot({ path: testInfo.outputPath('console-flat-1000x680-height-250.png') });

  await page.setViewportSize({ width: 722, height: 480 });
  await assertSeparatedFrame();
  await page.screenshot({ path: testInfo.outputPath('console-flat-722x480.png') });
});

test('command completion wraps responsively and keeps semantic selection through reflow', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1200, height: 760 });
  await page.goto('/fixture.html');
  const input = page.getByRole('combobox', { name: 'Loadbot command' });
  await input.fill('inspect r');
  await input.press('Tab');
  const field = page.getByRole('listbox', { name: 'Command completions' });
  const candidates = field.getByRole('option');
  await expect(candidates).toHaveCount(5);
  await input.press('Tab');
  const selectedLabel = await field.getByRole('option', { selected: true }).textContent();

  const layout = async () => field.evaluate((element) => {
    const items = [...element.querySelectorAll<HTMLElement>('[role="option"]')];
    const rows = new Set(items.map((item) => Math.round(item.getBoundingClientRect().top)));
    return {
      rows: rows.size,
      noHorizontalOverflow: element.scrollWidth <= element.clientWidth,
      selected: element.querySelector<HTMLElement>('[aria-selected="true"]')?.textContent,
    };
  });
  const wide = await layout();
  expect(wide.noHorizontalOverflow).toBe(true);

  await page.setViewportSize({ width: 722, height: 560 });
  const narrow = await layout();
  expect(narrow.rows).toBeGreaterThanOrEqual(wide.rows);
  expect(narrow.noHorizontalOverflow).toBe(true);
  expect(narrow.selected).toBe(selectedLabel);
  await page.screenshot({ path: testInfo.outputPath('command-completion-wrapped-722.png') });

  await input.press('Escape');
  await input.fill('inspect community/research-tools-with-a-long-project-name ');
  await input.press('Tab');
  const largeField = page.getByRole('listbox', { name: 'Command completions' });
  const bounds = await largeField.evaluate((element) => ({
    clientHeight: element.clientHeight,
    scrollHeight: element.scrollHeight,
    noHorizontalOverflow: element.scrollWidth <= element.clientWidth,
  }));
  expect(bounds.clientHeight).toBeLessThanOrEqual(108);
  expect(bounds.scrollHeight).toBeGreaterThan(bounds.clientHeight);
  expect(bounds.noHorizontalOverflow).toBe(true);
});

test('approved skins, focus, scrolling, resizing, and drawer preserve a usable menu', async ({ page }, testInfo) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/fixture.html');
  await expect(page.getByRole('heading', { name: 'Malware triage' })).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: testInfo.outputPath('main-1000x680.png') });
  const frame = page.getByRole('region', { name: 'Loadbot menu' });
  expect(await frame.evaluate((element) => getComputedStyle(element).borderImageSlice)).toBe('12 fill');
  const projects = page.getByRole('group', { name: 'Projects', exact: true });
  const first = projects.getByRole('button', { name: 're-toolkit personal' });
  const selectedSkin = await first.evaluate((element) => getComputedStyle(element).borderImageSource);
  await first.hover();
  expect(await first.evaluate((element) => getComputedStyle(element).borderImageSource)).toBe(selectedSkin);
  await first.focus();
  await page.keyboard.press('ArrowDown');
  await expect(projects.getByRole('button', { name: 'radio personal' })).toBeFocused();
  await expect(first).toHaveAttribute('aria-pressed', 'true');
  await page.screenshot({ path: testInfo.outputPath('keyboard-focus.png') });
  await page.keyboard.press('Enter');
  await expect(page.getByRole('heading', { name: 'Inspect recording' })).toBeVisible();
  await first.click();
  await page.getByLabel('Input folder *').focus();
  await page.screenshot({ path: testInfo.outputPath('input-focus.png') });
  await page.getByRole('button', { name: 'Use sample input folder' }).click();
  await page.getByRole('checkbox').check();
  await expect(page.getByRole('region', { name: 'Bottom workspace' })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('drawer-1000x680.png') });
  await page.getByRole('button', { name: 'Console', exact: true }).click();
  await expect(page.getByLabel('Input folder *')).toHaveValue('samples/');
  await expect(page.getByRole('checkbox')).toBeChecked();
  await page.setViewportSize({ width: 722, height: 480 });
  await page.screenshot({ path: testInfo.outputPath('main-722x480.png') });
  await page.setViewportSize({ width: 1000, height: 680 });
  await projects.getByRole('button', { name: 'research-tools-with-a-long-project-name community' }).click();
  const shortcuts = page.getByRole('group', { name: 'Shortcuts', exact: true });
  await shortcuts.getByRole('button').first().focus();
  await page.keyboard.press('End');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('heading', { name: 'Review sample 18' })).toBeVisible();
  expect(await shortcuts.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
  await shortcuts.getByRole('button').first().click();
  await page.screenshot({ path: testInfo.outputPath('long-labels.png') });
  for (const size of [{ width: 722, height: 480 }, { width: 420, height: 480 }]) {
    await page.setViewportSize(size);
    await page.getByRole('button', { name: 'Console', exact: true }).click();
    await expect(page.getByRole('region', { name: 'Bottom workspace' })).toBeVisible();
    await page.getByRole('button', { name: 'RUN SHORTCUT' }).scrollIntoViewIfNeeded();
    await expect(page.getByRole('button', { name: 'RUN SHORTCUT' })).toBeInViewport();
    await expect(page.getByRole('button', { name: 'Console', exact: true })).toBeInViewport();
    expect(await frame.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
    await page.screenshot({ path: testInfo.outputPath(`small-drawer-${size.width}.png`) });
    await page.getByRole('button', { name: 'Console', exact: true }).click();
  }
  expect(errors).toEqual([]);
});

test('three splitters drag, clamp, persist, and reset independently', async ({ page }) => {
  await page.goto('/fixture.html');
  const projects = page.getByRole('separator', { name: 'Resize projects pane' });
  const shortcuts = page.getByRole('separator', { name: 'Resize shortcuts and selected shortcut' });
  const terminal = page.getByRole('separator', { name: 'Resize console pane' });
  await expect(projects).toHaveAttribute('aria-valuenow', '260');
  await expect(shortcuts).toHaveAttribute('aria-valuenow', '210');
  await expect(terminal).toHaveAttribute('aria-valuenow', '140');
  const boundary = (await projects.boundingBox())!;
  await page.mouse.move(boundary.x + 2, boundary.y + 4);
  await page.mouse.down();
  await page.mouse.move(boundary.x + 82, boundary.y + 4);
  await page.mouse.up();
  await expect(projects).toHaveAttribute('aria-valuenow', '340');
  await shortcuts.focus();
  await page.keyboard.press('ArrowDown');
  await expect(shortcuts).toHaveAttribute('aria-valuenow', '222');
  await terminal.focus();
  await page.keyboard.press('ArrowUp');
  await expect(terminal).toHaveAttribute('aria-valuenow', '152');
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('loadbot.workspace.panes.v1')!))).toEqual({
    version: 1, projects: 340, shortcuts: 222, terminal: 152,
  });
  await page.reload();
  await expect(projects).toHaveAttribute('aria-valuenow', '340');
  await projects.dblclick();
  await expect(projects).toHaveAttribute('aria-valuenow', '260');
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('loadbot.workspace.panes.v1')!).projects)).toBe(260);
  await page.reload();
  await expect(projects).toHaveAttribute('aria-valuenow', '260');
});

test('development overlay owns bounds, close, and keyboard focus without Tauri', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1200, height: 850 });
  await page.goto('/embed.html');
  await expect(page.getByRole('heading', { name: 'Malware triage' })).toBeVisible();
  expect(await page.evaluate(() => '__TAURI_INTERNALS__' in window)).toBe(false);
  const container = page.locator('.embedding-container');
  const menu = page.getByRole('region', { name: 'Loadbot menu' });
  const parentBox = (await container.boundingBox())!;
  const menuBox = (await menu.boundingBox())!;
  expect(menuBox).toEqual(parentBox);
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: testInfo.outputPath('parent-overlay.png') });
  await page.getByRole('button', { name: 'Close Loadbot menu' }).click();
  await expect(page.getByRole('dialog')).not.toBeVisible();
  await expect(page.getByRole('button', { name: 'Open Loadbot overlay' })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('dialog')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).not.toBeVisible();
});
