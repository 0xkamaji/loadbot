// @vitest-environment node
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { expect, it } from 'vitest';

const root = fileURLToPath(new URL('../frontend/', import.meta.url));
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
