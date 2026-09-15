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
      const headless = /^(loadbot\/(contract|identity)|loadbot\/application\/(controller|sampleForms)|loadbot\/fixtures\/adapter)/.test(source);
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

  for (const html of [desktopHtml, defaultHtml]) {
    expect(html).toContain('/frontend/hosts/standalone.tsx');
    expect(html).not.toMatch(/fixture/i);
  }
  expect(standalone).toContain("from './realComposition'");
  expect(standalone).not.toMatch(/fixtureComposition|fixtureMenuDependencies/);
  expect(realComposition).toContain("from './tauriInventoryAdapter'");
  expect(realComposition).not.toMatch(/from ['"].*fixture/);

  expect(fixtureHtml).toContain('/frontend/hosts/fixture.tsx');
  expect(fixtureEntry).toContain("from './fixtureComposition'");
  expect(fixtureComposition).toContain("from '../loadbot/fixtures/adapter'");
});
