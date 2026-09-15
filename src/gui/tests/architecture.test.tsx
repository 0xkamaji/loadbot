// @vitest-environment node
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { expect, it } from 'vitest';

const root = fileURLToPath(new URL('../frontend/', import.meta.url));
const guiRoot = resolve(root, '..');
function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? sourceFiles(path) : /\.tsx?$/.test(path) ? [path] : [];
  });
}

it('keeps fixtures/hosts/transport out of views and capabilities out of UI primitives', () => {
  const violations: string[] = [];
  for (const file of sourceFiles(root)) {
    const source = relative(root, file).replaceAll('\\', '/');
    const tree = ts.createSourceFile(file, readFileSync(file, 'utf8'), ts.ScriptTarget.Latest, true);
    function check(specifier: string) {
      const target = specifier.startsWith('.')
        ? relative(root, resolve(file, '..', specifier)).replaceAll('\\', '/') : specifier;
      const message = `${source} -> ${target}`;
      if (!source.startsWith('hosts/') && target.startsWith('hosts/')) violations.push(message);
      if (!source.startsWith('hosts/') && !source.startsWith('loadbot/fixtures/') && target.includes('/fixtures/')) violations.push(message);
      if (source.startsWith('ui/') && target.startsWith('loadbot/')) violations.push(message);
      if (source.startsWith('loadbot/application/') && /^(ui\/|loadbot\/view\/)/.test(target)) violations.push(message);
      if (source.startsWith('loadbot/view/') && target.includes('useLoadbotApplication')) violations.push(message);
      if (!source.startsWith('hosts/') && /(@tauri-apps|^node:)/.test(target)) violations.push(message);
      const headless = /^(loadbot\/(contract|identity)|loadbot\/application\/(command|controller|sampleForms)|loadbot\/fixtures\/adapter)/.test(source);
      if (headless && /^(react|ui\/|loadbot\/view\/)/.test(target)) violations.push(message);
    }
    function visit(node: ts.Node) {
      if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier)) check(node.moduleSpecifier.text);
      if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword && node.arguments[0] && ts.isStringLiteral(node.arguments[0])) check(node.arguments[0].text);
      ts.forEachChild(node, visit);
    }
    visit(tree);
  }
  expect(violations).toEqual([]);
});

it('keeps the Console frame decorative and its interior on a stable surface', () => {
  const theme = readFileSync(join(root, 'ui/theme.css'), 'utf8');
  const drawer = theme.match(/\.lb-drawer\s*\{([^}]*)\}/)?.[1] ?? '';
  expect(drawer).toContain('border-image: var(--shared-terminal-panel) var(--shared-terminal-panel-slice) stretch;');
  expect(drawer).not.toMatch(/border-image:[^;]*\bfill\b/);
  expect(drawer).toContain('background-color: var(--lb-terminal-surface)');
  expect(drawer).toContain('background-clip: padding-box');
});

it('pins native/default and fixture entry points to distinct compositions', () => {
  const packageJson = JSON.parse(readFileSync(join(guiRoot, 'package.json'), 'utf8'));
  const tauri = JSON.parse(readFileSync(join(guiRoot, 'src-tauri/tauri.conf.json'), 'utf8'));
  const nativeWindow = tauri.app.windows.find((window: { label?: string }) => window.label === 'main');

  expect(packageJson.scripts.desktop).toBe('tauri dev');
  expect(tauri.build.beforeDevCommand).toBe('npm run dev');
  expect(new URL(tauri.build.devUrl).pathname).toBe('/');
  expect(nativeWindow.url).toBe('desktop.html');

  const desktopHtml = readFileSync(join(guiRoot, nativeWindow.url), 'utf8');
  const defaultHtml = readFileSync(join(guiRoot, 'index.html'), 'utf8');
  const fixtureHtml = readFileSync(join(guiRoot, 'fixture.html'), 'utf8');
  const standalone = readFileSync(join(root, 'hosts/standalone.tsx'), 'utf8');
  const realComposition = readFileSync(join(root, 'hosts/realComposition.ts'), 'utf8');
  const fixtureEntry = readFileSync(join(root, 'hosts/fixture.tsx'), 'utf8');
  const fixtureComposition = readFileSync(join(root, 'hosts/fixtureComposition.ts'), 'utf8');
  const tauriAdapter = readFileSync(join(root, 'hosts/tauriInventoryAdapter.ts'), 'utf8');
  const tauriLayoutStore = readFileSync(join(root, 'hosts/tauriWorkspaceLayoutStore.ts'), 'utf8');
  const command = readFileSync(join(root, 'loadbot/application/command.ts'), 'utf8');
  const tauriMain = readFileSync(join(guiRoot, 'src-tauri/src/main.rs'), 'utf8');
  const tauriBuild = readFileSync(join(guiRoot, 'src-tauri/build.rs'), 'utf8');
  const nativeCapability = JSON.parse(readFileSync(join(guiRoot, 'src-tauri/capabilities/main.json'), 'utf8'));
  const managementPermission = readFileSync(join(guiRoot, 'src-tauri/permissions/management.toml'), 'utf8');

  for (const html of [desktopHtml, defaultHtml]) {
    expect(html).toContain('/frontend/hosts/standalone.tsx');
    expect(html).not.toMatch(/fixture/i);
  }
  expect(standalone).toContain("from './realComposition'");
  expect(standalone).not.toMatch(/fixtureComposition|fixtureMenuDependencies/);
  expect(realComposition).toContain("from './tauriInventoryAdapter'");
  expect(realComposition).toContain("from './tauriWorkspaceLayoutStore'");
  expect(realComposition).not.toMatch(/from ['"].*fixture/);
  expect(tauriAdapter).toContain("invoke('open_loadbot_project', { catalog: project.catalog, tool: project.tool })");
  for (const command of ['read_loadbot_catalogs', 'add_loadbot_catalog', 'add_loadbot_project', 'add_loadbot_shortcut', 'sync_loadbot_catalog']) {
    expect(tauriAdapter).toContain(command);
    expect(tauriMain).toContain(command);
    expect(tauriBuild).toContain(`"${command}"`);
    expect(managementPermission).toContain(`"${command}"`);
  }
  expect(nativeCapability.permissions).toContain('manage-loadbot');
  expect(tauriMain).toContain('operations::catalog_add');
  expect(tauriMain).toContain('operations::tool_add');
  expect(tauriMain).toContain('operations::shortcut_add');
  expect(tauriMain).toContain('operations::catalog_sync');
  expect(tauriAdapter).toContain('new Channel<unknown>()');
  expect(tauriMain).toContain('Channel<BackendActivity>');
  expect(tauriMain).toContain('Notice::CatalogSyncRepositoryChecked');
  expect(tauriAdapter).not.toMatch(/explorer|xdg-open|filesystem|fixture/i);
  expect(tauriMain).toContain('launcher::resolve_project_directory');
  expect(tauriMain).toContain('open_loadbot_project');
  expect(tauriLayoutStore).toContain("invoke<unknown>('read_loadbot_workspace_layout')");
  expect(tauriLayoutStore).toContain("invoke('write_loadbot_workspace_layout', { contents })");
  expect(tauriMain).toContain('app.path().app_local_data_dir()');
  expect(command).not.toMatch(/@tauri-apps|child_process|Command::new|\.spawn\(|powershell|cmd\.exe|\b(?:bash|sh)\b/);
  expect(command).not.toMatch(/cli\/|CLI output|invoke\(/);

  expect(fixtureHtml).toContain('/frontend/hosts/fixture.tsx');
  expect(fixtureEntry).toContain("from './fixtureComposition'");
  expect(fixtureComposition).toContain("from '../loadbot/fixtures/adapter'");
  expect(fixtureComposition).not.toMatch(/tauriWorkspaceLayoutStore|workspace_layout/);
});
