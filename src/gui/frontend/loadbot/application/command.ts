import type { LoadbotProject, LoadbotShortcut } from '../contract';
import { shortcutTarget } from '../identity';

export interface CommandDefinitionSummary {
  readonly name: string;
  readonly usage: string;
  readonly summary: string;
}

export type CommandErrorCode =
  | 'empty'
  | 'unknown-command'
  | 'unsupported-syntax'
  | 'malformed-input'
  | 'usage'
  | 'inventory-unavailable'
  | 'project-not-found'
  | 'project-ambiguous'
  | 'shortcut-not-found'
  | 'shortcut-ambiguous';

export type CommandResult =
  | { readonly kind: 'help'; readonly commands: readonly CommandDefinitionSummary[] }
  | { readonly kind: 'projects'; readonly catalog?: string; readonly projects: readonly LoadbotProject[] }
  | { readonly kind: 'shortcuts'; readonly project: LoadbotProject; readonly shortcuts: readonly LoadbotShortcut[] }
  | { readonly kind: 'project'; readonly project: LoadbotProject }
  | { readonly kind: 'shortcut'; readonly project: LoadbotProject; readonly shortcut: LoadbotShortcut }
  | {
    readonly kind: 'error'; readonly code: CommandErrorCode; readonly input?: string; readonly usage?: string;
    readonly subject?: string; readonly choices?: readonly string[];
  };

export interface CommandContext {
  readonly inventoryStatus: 'loading' | 'ready' | 'error';
  readonly projects: readonly LoadbotProject[];
  readonly currentCatalog?: string;
  readonly selectedProject?: LoadbotProject;
}

export interface CommandCompletionCandidate {
  /** Stable semantic identity; presentation reflow must not change selection. */
  readonly id: string;
  /** Text inserted into the command token before quoting is applied. */
  readonly value: string;
  readonly label: string;
  readonly matchTerms?: readonly string[];
}

export interface CommandCompletion {
  readonly candidates: readonly CommandCompletionCandidate[];
  readonly range: { readonly start: number; readonly end: number };
  readonly quote?: "'" | '"';
}

export interface AppliedCommandCompletion {
  readonly input: string;
  readonly caret: number;
}

interface ParsedCommand { readonly tokens: readonly string[] }
type ParseResult = ParsedCommand | Extract<CommandResult, { kind: 'error' }>;
interface CommandDefinition extends CommandDefinitionSummary {
  readonly dispatch: (arguments_: readonly string[], context: CommandContext) => CommandResult;
  readonly complete?: (
    argumentIndex: number,
    argumentsBefore: readonly string[],
    context: CommandContext,
  ) => readonly CommandCompletionCandidate[];
}

const unsupportedShellSyntax = /\$\(|&&|\|\||[|&;`<>]/;

/** Tokenize only whitespace and quoted names. Shell operators are deliberately rejected. */
export function parseCommandLine(input: string): ParseResult {
  if (unsupportedShellSyntax.test(input)) return { kind: 'error', code: 'unsupported-syntax' };
  const tokens: string[] = [];
  let token = '';
  let quote: "'" | '"' | undefined;
  let hasToken = false;
  for (let index = 0; index < input.length; index++) {
    const character = input[index];
    if (quote) {
      if (character === '\\' && (input[index + 1] === quote || input[index + 1] === '\\')) {
        token += input[++index];
      } else if (character === quote) {
        quote = undefined;
      } else {
        token += character;
      }
      hasToken = true;
    } else if (character === '"' || character === "'") {
      quote = character;
      hasToken = true;
    } else if (/\s/.test(character)) {
      if (hasToken) {
        tokens.push(token);
        token = '';
        hasToken = false;
      }
    } else {
      token += character;
      hasToken = true;
    }
  }
  if (quote) return { kind: 'error', code: 'malformed-input' };
  if (hasToken) tokens.push(token);
  return { tokens };
}

const usage = (definition: CommandDefinition): CommandResult => ({ kind: 'error', code: 'usage', usage: definition.usage });
const unavailable = (context: CommandContext): CommandResult | undefined => context.inventoryStatus === 'ready'
  ? undefined
  : { kind: 'error', code: 'inventory-unavailable' };

function qualifiedProject(project: LoadbotProject): string {
  return `${project.catalog}/${project.tool}`;
}

function projectCompletionCandidates(context: CommandContext): readonly CommandCompletionCandidate[] {
  if (context.inventoryStatus !== 'ready') return [];
  const candidates: CommandCompletionCandidate[] = [];
  const usedValues = new Set<string>();
  for (const project of context.projects) {
    const sameName = context.projects.filter((candidate) => candidate.tool === project.tool);
    const contextual = sameName.filter((candidate) => candidate.catalog === context.currentCatalog);
    const unqualifiedIsSafe = context.currentCatalog
      ? project.catalog === context.currentCatalog && contextual.length === 1
      : sameName.length === 1;
    const value = unqualifiedIsSafe ? project.tool : qualifiedProject(project);
    if (usedValues.has(value)) continue;
    usedValues.add(value);
    candidates.push({
      id: qualifiedProject(project), value, label: value,
      matchTerms: value === project.tool ? [value] : [value, project.tool],
    });
  }
  return candidates;
}

export function resolveProject(reference: string, context: CommandContext): LoadbotProject | CommandResult {
  const separator = reference.indexOf('/');
  if (separator > 0 && separator < reference.length - 1) {
    const catalog = reference.slice(0, separator);
    const tool = reference.slice(separator + 1);
    return context.projects.find((project) => project.catalog === catalog && project.tool === tool)
      ?? { kind: 'error', code: 'project-not-found', subject: reference };
  }
  const matches = context.projects.filter((project) => project.tool === reference);
  const contextual = matches.filter((project) => project.catalog === context.currentCatalog);
  if (contextual.length === 1) return contextual[0];
  if (contextual.length > 1) return {
    kind: 'error', code: 'project-ambiguous', subject: reference, choices: contextual.map(qualifiedProject),
  };
  if (!context.currentCatalog && matches.length === 1) return matches[0];
  if (matches.length > 0) return {
    kind: 'error', code: matches.length > 1 ? 'project-ambiguous' : 'project-not-found', subject: reference,
    choices: matches.map(qualifiedProject),
  };
  return { kind: 'error', code: 'project-not-found', subject: reference };
}

function qualifiedShortcut(shortcut: LoadbotShortcut): string {
  return `${shortcut.source}::${shortcut.name}::${shortcutTarget(shortcut)}`;
}

function shortcutCompletionCandidates(project: LoadbotProject): readonly CommandCompletionCandidate[] {
  return project.entries.map((shortcut) => {
    const qualified = qualifiedShortcut(shortcut);
    const duplicate = project.entries.filter((candidate) => candidate.name === shortcut.name).length > 1;
    const value = duplicate ? qualified : shortcut.name;
    return { id: qualified, value, label: value, matchTerms: value === shortcut.name ? [value] : [value, shortcut.name] };
  });
}

function resolveShortcut(reference: string, project: LoadbotProject): LoadbotShortcut | CommandResult {
  const [possibleSource, possibleName, ...path] = reference.split('::');
  const qualified = (possibleSource === 'catalog' || possibleSource === 'personal') && possibleName !== undefined;
  const matches = project.entries.filter((shortcut) => qualified
    ? shortcut.source === possibleSource && shortcut.name === possibleName && (!path.length || shortcutTarget(shortcut) === path.join('::'))
    : shortcut.name === reference);
  if (matches.length === 1) return matches[0];
  if (!matches.length) return { kind: 'error', code: 'shortcut-not-found', subject: reference };
  return {
    kind: 'error', code: 'shortcut-ambiguous', subject: reference, choices: matches.map(qualifiedShortcut),
  };
}

const definitions: readonly CommandDefinition[] = [
  {
    name: 'help', usage: 'help', summary: 'Show available Loadbot commands.',
    dispatch(arguments_, _context) { return arguments_.length ? usage(this) : { kind: 'help', commands: commandDefinitions }; },
  },
  {
    name: 'projects', usage: 'projects', summary: 'List projects in the current catalog.',
    dispatch(arguments_, context) {
      if (arguments_.length) return usage(this);
      return unavailable(context) ?? {
        kind: 'projects', catalog: context.currentCatalog,
        projects: context.currentCatalog
          ? context.projects.filter((project) => project.catalog === context.currentCatalog)
          : context.projects,
      };
    },
  },
  {
    name: 'shortcuts', usage: 'shortcuts [project]', summary: 'List shortcuts for a project.',
    complete(argumentIndex, _argumentsBefore, context) {
      return argumentIndex === 0 ? projectCompletionCandidates(context) : [];
    },
    dispatch(arguments_, context) {
      if (arguments_.length > 1) return usage(this);
      const unavailableResult = unavailable(context);
      if (unavailableResult) return unavailableResult;
      const project = arguments_[0] ? resolveProject(arguments_[0], context) : context.selectedProject;
      if (!project) return { kind: 'error', code: 'usage', usage: this.usage };
      if ('kind' in project) return project;
      return { kind: 'shortcuts', project, shortcuts: project.entries };
    },
  },
  {
    name: 'inspect', usage: 'inspect <project> [shortcut]', summary: 'Inspect a project or one of its shortcuts.',
    complete(argumentIndex, argumentsBefore, context) {
      if (argumentIndex === 0) return projectCompletionCandidates(context);
      if (argumentIndex !== 1 || !argumentsBefore[0] || context.inventoryStatus !== 'ready') return [];
      const project = resolveProject(argumentsBefore[0], context);
      return 'kind' in project ? [] : shortcutCompletionCandidates(project);
    },
    dispatch(arguments_, context) {
      if (!arguments_.length || arguments_.length > 2) return usage(this);
      const unavailableResult = unavailable(context);
      if (unavailableResult) return unavailableResult;
      const project = resolveProject(arguments_[0], context);
      if ('kind' in project) return project;
      if (!arguments_[1]) return { kind: 'project', project };
      const shortcut = resolveShortcut(arguments_[1], project);
      return 'kind' in shortcut ? shortcut : { kind: 'shortcut', project, shortcut };
    },
  },
  {
    name: 'pull', usage: 'pull <project>', summary: 'Pull/install a managed tool.',
    complete(argumentIndex, _argumentsBefore, context) {
      return argumentIndex === 0 ? projectCompletionCandidates(context) : [];
    },
    dispatch(arguments_, context) {
      if (arguments_.length !== 1) return usage(this);
      const unavailableResult = unavailable(context);
      if (unavailableResult) return unavailableResult;
      const project = resolveProject(arguments_[0], context);
      if ('kind' in project) return project;
      return { kind: 'error', code: 'unsupported-syntax', input: arguments_.join(' '), subject: 'pull requires Tauri backend' };
    },
  },
  {
    name: 'update', usage: 'update <project>', summary: 'Update a managed tool.',
    complete(argumentIndex, _argumentsBefore, context) {
      return argumentIndex === 0 ? projectCompletionCandidates(context) : [];
    },
    dispatch(arguments_, context) {
      if (arguments_.length !== 1) return usage(this);
      const unavailableResult = unavailable(context);
      if (unavailableResult) return unavailableResult;
      const project = resolveProject(arguments_[0], context);
      if ('kind' in project) return project;
      return { kind: 'error', code: 'unsupported-syntax', input: arguments_.join(' '), subject: 'update requires Tauri backend' };
    },
  },
  {
    name: 'push', usage: 'push <project>', summary: 'Push local commits for a managed tool.',
    complete(argumentIndex, _argumentsBefore, context) {
      return argumentIndex === 0 ? projectCompletionCandidates(context) : [];
    },
    dispatch(arguments_, context) {
      if (arguments_.length !== 1) return usage(this);
      const unavailableResult = unavailable(context);
      if (unavailableResult) return unavailableResult;
      const project = resolveProject(arguments_[0], context);
      if ('kind' in project) return project;
      return { kind: 'error', code: 'unsupported-syntax', input: arguments_.join(' '), subject: 'push requires Tauri backend' };
    },
  },
  {
    name: 'remove', usage: 'remove <project>', summary: 'Remove a managed tool checkout.',
    complete(argumentIndex, _argumentsBefore, context) {
      return argumentIndex === 0 ? projectCompletionCandidates(context) : [];
    },
    dispatch(arguments_, context) {
      if (arguments_.length !== 1) return usage(this);
      const unavailableResult = unavailable(context);
      if (unavailableResult) return unavailableResult;
      const project = resolveProject(arguments_[0], context);
      if ('kind' in project) return project;
      return { kind: 'error', code: 'unsupported-syntax', input: arguments_.join(' '), subject: 'remove requires Tauri backend' };
    },
  },
  {
    name: 'reinstall', usage: 'reinstall <project>', summary: 'Reinstall a managed tool.',
    complete(argumentIndex, _argumentsBefore, context) {
      return argumentIndex === 0 ? projectCompletionCandidates(context) : [];
    },
    dispatch(arguments_, context) {
      if (arguments_.length !== 1) return usage(this);
      const unavailableResult = unavailable(context);
      if (unavailableResult) return unavailableResult;
      const project = resolveProject(arguments_[0], context);
      if ('kind' in project) return project;
      return { kind: 'error', code: 'unsupported-syntax', input: arguments_.join(' '), subject: 'reinstall requires Tauri backend' };
    },
  },
  {
    name: 'status', usage: 'status <project>', summary: 'Show status of a managed tool.',
    complete(argumentIndex, _argumentsBefore, context) {
      return argumentIndex === 0 ? projectCompletionCandidates(context) : [];
    },
    dispatch(arguments_, context) {
      if (arguments_.length !== 1) return usage(this);
      const unavailableResult = unavailable(context);
      if (unavailableResult) return unavailableResult;
      const project = resolveProject(arguments_[0], context);
      if ('kind' in project) return project;
      return { kind: 'error', code: 'unsupported-syntax', input: arguments_.join(' '), subject: 'status requires Tauri backend' };
    },
  },
];

export const commandDefinitions: readonly CommandDefinitionSummary[] = definitions.map(({ name, usage: commandUsage, summary }) => ({
  name, usage: commandUsage, summary,
}));

interface CompletionToken {
  readonly start: number;
  readonly end: number;
  readonly value: string;
  readonly quote?: "'" | '"';
  readonly malformed: boolean;
}

/** Scan token boundaries without relaxing the parser used for command execution. */
function completionTokens(input: string): readonly CompletionToken[] {
  const tokens: CompletionToken[] = [];
  let start: number | undefined;
  let value = '';
  let quote: "'" | '"' | undefined;
  let leadingQuote: "'" | '"' | undefined;
  for (let index = 0; index < input.length; index++) {
    const character = input[index];
    if (quote) {
      if (character === '\\' && (input[index + 1] === quote || input[index + 1] === '\\')) value += input[++index];
      else if (character === quote) quote = undefined;
      else value += character;
      continue;
    }
    if (character === '"' || character === "'") {
      if (start === undefined) {
        start = index;
        leadingQuote = character;
      }
      quote = character;
    } else if (/\s/.test(character)) {
      if (start !== undefined) {
        tokens.push({ start, end: index, value, quote: leadingQuote, malformed: false });
        start = undefined;
        value = '';
        leadingQuote = undefined;
      }
    } else {
      if (start === undefined) start = index;
      value += character;
    }
  }
  if (start !== undefined) tokens.push({ start, end: input.length, value, quote: leadingQuote, malformed: quote !== undefined });
  return tokens;
}

function decodedPrefix(input: string, token: CompletionToken, caret: number): string | undefined {
  const raw = input.slice(token.start, caret);
  let value = '';
  let quote: "'" | '"' | undefined;
  for (let index = 0; index < raw.length; index++) {
    const character = raw[index];
    if (quote) {
      if (character === '\\' && (raw[index + 1] === quote || raw[index + 1] === '\\')) value += raw[++index];
      else if (character === quote) quote = undefined;
      else value += character;
    } else if (character === '"' || character === "'") quote = character;
    else if (/\s/.test(character)) return undefined;
    else value += character;
  }
  return value;
}

/** Return semantic candidates for the token at the caret. No adapter or process is invoked. */
export function completeLoadbotCommand(input: string, caret: number, context: CommandContext): CommandCompletion | undefined {
  if (caret < 0 || caret > input.length || unsupportedShellSyntax.test(input)) return undefined;
  const tokens = completionTokens(input);
  // At an end boundary, complete the token immediately left of the caret. A caret
  // after actual separating whitespace is outside that token and starts a new one.
  const tokenIndex = tokens.findIndex((token) => caret >= token.start && caret <= token.end);
  const active = tokenIndex >= 0 ? tokens[tokenIndex] : undefined;
  const range = active ? { start: active.start, end: active.end } : { start: caret, end: caret };
  const insertionIndex = active ? tokenIndex : tokens.filter((token) => token.end <= caret).length;
  const prior = tokens.slice(0, insertionIndex);
  if (prior.some((token) => token.malformed)) return undefined;
  const prefix = active ? decodedPrefix(input, active, caret) : '';
  if (prefix === undefined) return undefined;

  let candidates: readonly CommandCompletionCandidate[];
  if (insertionIndex === 0) {
    candidates = definitions.map(({ name }) => ({ id: `command:${name}`, value: name, label: name }));
  } else {
    const definition = definitions.find((candidate) => candidate.name === prior[0]?.value.toLowerCase());
    candidates = definition?.complete?.(insertionIndex - 1, prior.slice(1).map((token) => token.value), context) ?? [];
  }
  const lowered = prefix.toLocaleLowerCase();
  const matches = candidates.filter((candidate) => (candidate.matchTerms ?? [candidate.value])
    .some((term) => term.toLocaleLowerCase().startsWith(lowered)));
  return matches.length ? { candidates: matches, range, quote: active?.quote } : undefined;
}

function quotedCompletionValue(value: string, quote?: "'" | '"'): string {
  const selectedQuote = quote ?? (/\s/.test(value) ? '"' : undefined);
  if (!selectedQuote) return value;
  const escaped = value.replaceAll('\\', '\\\\').replaceAll(selectedQuote, `\\${selectedQuote}`);
  return `${selectedQuote}${escaped}${selectedQuote}`;
}

/** Apply one candidate to only the active token and return the new caret position. */
export function applyCommandCompletion(
  input: string,
  completion: Pick<CommandCompletion, 'range' | 'quote'>,
  candidate: CommandCompletionCandidate,
): AppliedCommandCompletion {
  const replacement = quotedCompletionValue(candidate.value, completion.quote);
  const completed = `${input.slice(0, completion.range.start)}${replacement}${input.slice(completion.range.end)}`;
  return { input: completed, caret: completion.range.start + replacement.length };
}

export function executeLoadbotCommand(input: string, context: CommandContext): CommandResult {
  const parsed = parseCommandLine(input);
  if ('kind' in parsed) return parsed;
  if (!parsed.tokens.length) return { kind: 'error', code: 'empty' };
  const [name, ...arguments_] = parsed.tokens;
  const definition = definitions.find((candidate) => candidate.name === name.toLowerCase());
  return definition?.dispatch(arguments_, context)
    ?? { kind: 'error', code: 'unknown-command', input: name };
}
