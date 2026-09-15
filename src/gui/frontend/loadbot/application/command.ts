import type { LoadbotProject, LoadbotShortcut } from '../contract';

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

interface ParsedCommand { readonly tokens: readonly string[] }
type ParseResult = ParsedCommand | Extract<CommandResult, { kind: 'error' }>;
interface CommandDefinition extends CommandDefinitionSummary {
  readonly dispatch: (arguments_: readonly string[], context: CommandContext) => CommandResult;
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

function resolveProject(reference: string, context: CommandContext): LoadbotProject | CommandResult {
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
  return `${shortcut.source}::${shortcut.name}::${shortcut.path}`;
}

function resolveShortcut(reference: string, project: LoadbotProject): LoadbotShortcut | CommandResult {
  const [possibleSource, possibleName, ...path] = reference.split('::');
  const qualified = (possibleSource === 'catalog' || possibleSource === 'personal') && possibleName !== undefined;
  const matches = project.entries.filter((shortcut) => qualified
    ? shortcut.source === possibleSource && shortcut.name === possibleName && (!path.length || shortcut.path === path.join('::'))
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
];

export const commandDefinitions: readonly CommandDefinitionSummary[] = definitions.map(({ name, usage: commandUsage, summary }) => ({
  name, usage: commandUsage, summary,
}));

export function executeLoadbotCommand(input: string, context: CommandContext): CommandResult {
  const parsed = parseCommandLine(input);
  if ('kind' in parsed) return parsed;
  if (!parsed.tokens.length) return { kind: 'error', code: 'empty' };
  const [name, ...arguments_] = parsed.tokens;
  const definition = definitions.find((candidate) => candidate.name === name.toLowerCase());
  return definition?.dispatch(arguments_, context)
    ?? { kind: 'error', code: 'unknown-command', input: name };
}
