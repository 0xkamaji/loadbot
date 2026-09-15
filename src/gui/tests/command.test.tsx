// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { commandDefinitions, executeLoadbotCommand, parseCommandLine, type CommandContext } from '../frontend/loadbot/application/command';
import type { LoadbotProject } from '../frontend/loadbot/contract';

const projects: readonly LoadbotProject[] = [
  {
    catalog: 'personal', tool: 'Project One', entries: [
      { name: 'Export strings', path: 'catalog/export.py', source: 'catalog', description: 'Shared exporter', runner: 'python' },
      { name: 'Export strings', path: 'personal/export.py', source: 'personal' },
      { name: 'Unique shortcut', path: 'run.sh', source: 'personal', runner: 'sh' },
    ],
  },
  { catalog: 'community', tool: 'Project One', entries: [] },
  { catalog: 'personal', tool: 'solo', entries: [] },
];
const context: CommandContext = {
  inventoryStatus: 'ready', projects, currentCatalog: 'personal', selectedProject: projects[0],
};

describe('Loadbot command parser and registry', () => {
  it('tokenizes whitespace and quoted names without shell expansion', () => {
    expect(parseCommandLine('  inspect   "Project One"  \'Unique shortcut\' ')).toEqual({
      tokens: ['inspect', 'Project One', 'Unique shortcut'],
    });
    expect(parseCommandLine('inspect "Project \\"One\\""')).toEqual({ tokens: ['inspect', 'Project "One"'] });
    expect(parseCommandLine('inspect "unterminated')).toEqual({ kind: 'error', code: 'malformed-input' });
  });

  it('generates help from the registered working commands', () => {
    const result = executeLoadbotCommand('help', context);
    expect(result).toEqual({ kind: 'help', commands: commandDefinitions });
    expect(commandDefinitions.map((command) => command.name)).toEqual(['help', 'projects', 'shortcuts', 'inspect']);
    expect(executeLoadbotCommand('help extra', context)).toMatchObject({ kind: 'error', code: 'usage', usage: 'help' });
  });

  it('lists projects and shortcuts from the semantic inventory context', () => {
    const projectResult = executeLoadbotCommand('projects', context);
    expect(projectResult.kind === 'projects' && projectResult.projects.map((project) => project.tool)).toEqual(['Project One', 'solo']);
    const selected = executeLoadbotCommand('shortcuts', context);
    expect(selected.kind === 'shortcuts' && selected.shortcuts).toHaveLength(3);
    const named = executeLoadbotCommand('shortcuts "Project One"', context);
    expect(named.kind === 'shortcuts' && named.project.catalog).toBe('personal');
    const qualified = executeLoadbotCommand('shortcuts "community/Project One"', context);
    expect(qualified.kind === 'shortcuts' && qualified.project.catalog).toBe('community');
  });

  it('inspects real project and shortcut facts and preserves qualified identities', () => {
    expect(executeLoadbotCommand('inspect "Project One"', context)).toMatchObject({
      kind: 'project', project: { catalog: 'personal', tool: 'Project One' },
    });
    expect(executeLoadbotCommand('inspect "Project One" "Unique shortcut"', context)).toMatchObject({
      kind: 'shortcut', shortcut: { name: 'Unique shortcut', source: 'personal', path: 'run.sh' },
    });
    expect(executeLoadbotCommand('inspect "Project One" "personal::Export strings::personal/export.py"', context)).toMatchObject({
      kind: 'shortcut', shortcut: { source: 'personal', path: 'personal/export.py' },
    });
  });

  it('reports missing, excess, unknown, and ambiguous identities without guessing', () => {
    expect(executeLoadbotCommand('inspect', context)).toMatchObject({ kind: 'error', code: 'usage' });
    expect(executeLoadbotCommand('inspect one two three', context)).toMatchObject({ kind: 'error', code: 'usage' });
    expect(executeLoadbotCommand('shortcuts one two', context)).toMatchObject({ kind: 'error', code: 'usage' });
    expect(executeLoadbotCommand('wat', context)).toEqual({ kind: 'error', code: 'unknown-command', input: 'wat' });
    expect(executeLoadbotCommand('inspect missing', context)).toEqual({ kind: 'error', code: 'project-not-found', subject: 'missing' });
    expect(executeLoadbotCommand('inspect "Project One" "Export strings"', context)).toEqual({
      kind: 'error', code: 'shortcut-ambiguous', subject: 'Export strings',
      choices: ['catalog::Export strings::catalog/export.py', 'personal::Export strings::personal/export.py'],
    });
    expect(executeLoadbotCommand('inspect "Project One"', { ...context, currentCatalog: undefined })).toEqual({
      kind: 'error', code: 'project-ambiguous', subject: 'Project One',
      choices: ['personal/Project One', 'community/Project One'],
    });
  });

  it('rejects shell syntax and treats operating-system commands as unknown Loadbot commands', () => {
    for (const input of ['echo foo | bar', 'foo && bar', 'foo || bar', 'projects > out', '$(whoami)', '`whoami`', 'foo; bar']) {
      expect(executeLoadbotCommand(input, context)).toEqual({ kind: 'error', code: 'unsupported-syntax' });
    }
    for (const input of ['ls', 'dir', 'powershell', 'bash', 'rm -rf nowhere']) {
      expect(executeLoadbotCommand(input, context)).toMatchObject({ kind: 'error', code: 'unknown-command' });
    }
  });

  it('keeps help available while refusing inventory commands when the authoritative read is unavailable', () => {
    const loading = { ...context, inventoryStatus: 'loading' as const, projects: [] };
    expect(executeLoadbotCommand('help', loading).kind).toBe('help');
    expect(executeLoadbotCommand('projects', loading)).toEqual({ kind: 'error', code: 'inventory-unavailable' });
  });
});
