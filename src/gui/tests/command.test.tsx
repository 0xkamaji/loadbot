// @vitest-environment node
import { describe, expect, it } from 'vitest';
import {
  applyCommandCompletion, commandDefinitions, completeLoadbotCommand, executeLoadbotCommand, parseCommandLine,
  type CommandContext,
} from '../frontend/loadbot/application/command';
import type { LoadbotProject } from '../frontend/loadbot/contract';

const projects: readonly LoadbotProject[] = [
  { catalog: 'personal', tool: 'Project One', entries: [] },
  { catalog: 'community', tool: 'Project One', entries: [] },
  { catalog: 'personal', tool: 'solo', entries: [] },
];
const context: CommandContext = {
  inventoryStatus: 'ready', projects, currentCatalog: 'personal',
};

describe('Loadbot command parser and registry', () => {
  it('tokenizes whitespace and quoted names without shell expansion', () => {
    expect(parseCommandLine('  inspect   "Project One" ')).toEqual({
      tokens: ['inspect', 'Project One'],
    });
    expect(parseCommandLine('inspect "Project \\"One\\""')).toEqual({ tokens: ['inspect', 'Project "One"'] });
    expect(parseCommandLine('inspect "unterminated')).toEqual({ kind: 'error', code: 'malformed-input' });
  });

  it('generates help from the registered working commands', () => {
    const result = executeLoadbotCommand('help', context);
    expect(result).toEqual({ kind: 'help', commands: commandDefinitions });
    expect(commandDefinitions.map((command) => command.name)).toEqual(['help', 'projects', 'inspect', 'pull', 'update', 'push', 'remove', 'reinstall']);
    expect(executeLoadbotCommand('help extra', context)).toMatchObject({ kind: 'error', code: 'usage', usage: 'help' });
  });

  it('lists projects from the semantic inventory context', () => {
    const projectResult = executeLoadbotCommand('projects', context);
    expect(projectResult.kind === 'projects' && projectResult.projects.map((project) => project.tool)).toEqual(['Project One', 'solo']);
  });

  it('inspects project facts and preserves qualified identities', () => {
    expect(executeLoadbotCommand('inspect "Project One"', context)).toMatchObject({
      kind: 'project', project: { catalog: 'personal', tool: 'Project One' },
    });
    expect(executeLoadbotCommand('inspect "community/Project One"', context)).toMatchObject({
      kind: 'project', project: { catalog: 'community', tool: 'Project One' },
    });
  });

  it('reports missing, excess, unknown, and ambiguous identities without guessing', () => {
    expect(executeLoadbotCommand('inspect', context)).toMatchObject({ kind: 'error', code: 'usage' });
    expect(executeLoadbotCommand('inspect one two three', context)).toMatchObject({ kind: 'error', code: 'usage' });
    expect(executeLoadbotCommand('shortcuts', context)).toEqual({ kind: 'error', code: 'unknown-command', input: 'shortcuts' });
    expect(executeLoadbotCommand('wat', context)).toEqual({ kind: 'error', code: 'unknown-command', input: 'wat' });
    expect(executeLoadbotCommand('inspect missing', context)).toEqual({ kind: 'error', code: 'project-not-found', subject: 'missing' });
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

describe('Loadbot semantic command completion', () => {
  it('derives deterministic first-token candidates from the command registry', () => {
    const all = completeLoadbotCommand('', 0, context);
    expect(all?.candidates.map((candidate) => candidate.value)).toEqual(commandDefinitions.map((command) => command.name));
    expect(completeLoadbotCommand('sho', 3, context)).toBeUndefined();
  });

  it('offers safe project identities, including qualified ambiguous projects', () => {
    const completion = completeLoadbotCommand('inspect Pro', 11, context);
    expect(completion?.candidates.map((candidate) => candidate.value)).toEqual(['Project One', 'community/Project One']);
    expect(applyCommandCompletion('inspect Pro', completion!, completion!.candidates[0])).toEqual({
      input: 'inspect "Project One"', caret: 21,
    });
    expect(completeLoadbotCommand('inspect comm', 12, context)?.candidates.map((candidate) => candidate.value))
      .toEqual(['community/Project One']);
    expect(completeLoadbotCommand('inspect solo extra', 18, context)).toBeUndefined();
  });

  it('replaces only the active token while preserving quotes and surrounding text', () => {
    const middleInput = 'inspect so trailing';
    const middle = completeLoadbotCommand(middleInput, 10, context)!;
    expect(applyCommandCompletion(middleInput, middle, middle.candidates[0])).toEqual({
      input: 'inspect solo trailing', caret: 12,
    });

    const quotedInput = "inspect 'Pro";
    const quoted = completeLoadbotCommand(quotedInput, quotedInput.length, context)!;
    expect(applyCommandCompletion(quotedInput, quoted, quoted.candidates[0])).toEqual({
      input: "inspect 'Project One'", caret: 21,
    });
    expect(parseCommandLine(applyCommandCompletion(quotedInput, quoted, quoted.candidates[0]).input))
      .toEqual({ tokens: ['inspect', 'Project One'] });
  });

  it('returns no candidates for no-match, excess, unsafe, or malformed context', () => {
    expect(completeLoadbotCommand('inspect missing', 15, context)).toBeUndefined();
    expect(completeLoadbotCommand('projects anything', 17, context)).toBeUndefined();
    expect(completeLoadbotCommand('echo foo | ba', 13, context)).toBeUndefined();
    expect(completeLoadbotCommand('inspect "Project One" missing', 29, context)).toBeUndefined();
    expect(completeLoadbotCommand('sho', 99, context)).toBeUndefined();
  });
});

describe('Lifecycle command project resolution', () => {
  it('lifecycle commands resolve projects from command text', () => {
    const ctx = { ...context };
    for (const action of ['pull', 'update', 'push', 'remove', 'reinstall'] as const) {
      expect(executeLoadbotCommand(`${action} "Project One"`, ctx)).toEqual({
        kind: 'lifecycle', action, project: projects[0],
      });
    }
  });
});
