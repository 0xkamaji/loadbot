import type { LoadbotRecipe, LoadbotRecipeArgument, LoadbotRunner, LoadbotShortcut } from '../contract';

export type RecipeParameterKind = 'text' | 'file' | 'directory' | 'switch' | 'prefixed-input' | 'literal';
export interface RecipeDraftArgument {
  readonly key: number;
  readonly value: LoadbotRecipeArgument;
  readonly idManuallyEdited: boolean;
}
export interface RecipeDraft {
  readonly mode: 'create' | 'edit';
  readonly name: string;
  readonly description: string;
  readonly target: string;
  readonly runner: LoadbotRunner;
  readonly runnerManuallyEdited: boolean;
  readonly workingDirectory: LoadbotRecipe['working_directory'];
  readonly arguments: readonly RecipeDraftArgument[];
}

export const suggestedParameterId = (label: string) => label.trim().toLowerCase()
  .replace(/[^a-z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '') || 'parameter';

/** A conservative convenience only; the user remains authoritative. */
export function suggestedRunner(target: string): LoadbotRunner {
  const normalized = target.trim().toLowerCase();
  if (normalized.endsWith('.py')) return 'python';
  if (normalized.endsWith('.ps1')) return 'powershell';
  if (normalized.endsWith('.sh')) return 'bash';
  return 'direct';
}

export function newRecipeDraft(): RecipeDraft {
  return {
    mode: 'create', name: '', description: '', target: '', runner: 'direct', runnerManuallyEdited: false,
    workingDirectory: { type: 'project-root' }, arguments: [],
  };
}

/** Only Recipes that this form can round-trip without loss are editable. */
export function recipeDraftFromShortcut(shortcut: LoadbotShortcut): RecipeDraft | undefined {
  const recipe = shortcut.recipe;
  if (!recipe || shortcut.source !== 'personal' || recipe.behavior !== 'run') return undefined;
  let target: string;
  let runner: LoadbotRunner;
  let parameters: readonly LoadbotRecipeArgument[];
  if (recipe.program.type === 'project-file') {
    target = recipe.program.path;
    runner = 'direct';
    parameters = recipe.arguments;
  } else if (recipe.program.type === 'interpreter' && recipe.arguments[0]?.type === 'project-path') {
    target = recipe.arguments[0].path;
    runner = recipe.program.runner;
    parameters = recipe.arguments.slice(1);
  } else return undefined;
  return {
    mode: 'edit', name: shortcut.name, description: shortcut.description ?? '', target, runner,
    runnerManuallyEdited: true, workingDirectory: { ...recipe.working_directory },
    arguments: parameters.map((value, key) => ({ key, value: { ...value }, idManuallyEdited: true })),
  };
}

export function updateDraftTarget(draft: RecipeDraft, target: string): RecipeDraft {
  const runner = draft.runnerManuallyEdited ? draft.runner : suggestedRunner(target);
  const workingDirectory = draft.workingDirectory.type === 'target-parent' && runner !== 'direct'
    ? { type: 'project-root' as const } : draft.workingDirectory;
  return { ...draft, target, runner, workingDirectory };
}

export function updateDraftRunner(draft: RecipeDraft, runner: LoadbotRunner): RecipeDraft {
  const workingDirectory = draft.workingDirectory.type === 'target-parent' && runner !== 'direct'
    ? { type: 'project-root' as const } : draft.workingDirectory;
  return { ...draft, runner, runnerManuallyEdited: true, workingDirectory };
}

export function addDraftArgument(draft: RecipeDraft, kind: RecipeParameterKind, key: number): RecipeDraft {
  const value: LoadbotRecipeArgument = kind === 'literal' ? { type: 'literal', value: '' }
    : kind === 'switch' ? { type: 'switch', id: `flag-${key}`, label: 'Flag', value: '', default: false }
      : { type: 'input', id: `parameter-${key}`, label: kind === 'prefixed-input' ? 'Value' : kind[0].toUpperCase() + kind.slice(1),
        kind: kind === 'file' || kind === 'directory' ? kind : 'text', required: false,
        ...(kind === 'prefixed-input' ? { prefix: '' } : {}) };
  return { ...draft, arguments: [...draft.arguments, { key, value, idManuallyEdited: false }] };
}

export function updateDraftArgument(draft: RecipeDraft, key: number, value: LoadbotRecipeArgument, idManuallyEdited?: boolean): RecipeDraft {
  return { ...draft, arguments: draft.arguments.map((argument) => argument.key === key
    ? { ...argument, value, idManuallyEdited: idManuallyEdited ?? argument.idManuallyEdited } : argument) };
}

export function moveDraftArgument(draft: RecipeDraft, key: number, direction: -1 | 1): RecipeDraft {
  const index = draft.arguments.findIndex((argument) => argument.key === key);
  const target = index + direction;
  if (index < 0 || target < 0 || target >= draft.arguments.length) return draft;
  const arguments_ = [...draft.arguments];
  [arguments_[index], arguments_[target]] = [arguments_[target]!, arguments_[index]!];
  return { ...draft, arguments: arguments_ };
}

export function removeDraftArgument(draft: RecipeDraft, key: number): RecipeDraft {
  return { ...draft, arguments: draft.arguments.filter((argument) => argument.key !== key) };
}

export function recipeFromDraft(draft: RecipeDraft): LoadbotRecipe {
  const target: LoadbotRecipeArgument = { type: 'project-path', path: draft.target };
  return {
    version: 1,
    behavior: 'run',
    program: draft.runner === 'direct'
      ? { type: 'project-file', path: draft.target }
      : { type: 'interpreter', runner: draft.runner },
    working_directory: draft.workingDirectory,
    arguments: draft.runner === 'direct'
      ? draft.arguments.map((argument) => argument.value)
      : [target, ...draft.arguments.map((argument) => argument.value)],
  };
}

export function draftValidation(draft: RecipeDraft): readonly string[] {
  const errors: string[] = [];
  if (!draft.name.trim()) errors.push('Shortcut name is required.');
  if (!draft.target.trim()) errors.push('Target is required.');
  if (draft.workingDirectory.type === 'project-relative' && !draft.workingDirectory.path.trim()) errors.push('Run from folder is required.');
  if (draft.workingDirectory.type === 'target-parent' && draft.runner !== 'direct') errors.push("Target's folder is only available when running the target directly.");
  const ids = new Set<string>();
  for (const argument of draft.arguments) {
    const value = argument.value;
    if (value.type === 'project-path' && !value.path.trim()) errors.push('Fixed project path is required.');
    if (value.type === 'input' || value.type === 'switch') {
      if (!value.label.trim()) errors.push('Parameter name is required.');
      if (!value.id.trim()) errors.push('Parameter ID is required.');
      else if (ids.has(value.id)) errors.push(`Duplicate parameter ID: ${value.id}`);
      ids.add(value.id);
    }
    if (value.type === 'switch' && !value.value) errors.push(`Flag argument is required for ${value.label || value.id}.`);
    if (value.type === 'input' && value.prefix === '') errors.push(`Flag is required for ${value.label || value.id}.`);
  }
  return [...new Set(errors)];
}

export function recipePreview(draft: RecipeDraft): string {
  const parts = draft.runner === 'direct'
    ? [draft.target || '<target>']
    : [draft.runner, draft.target || '<target>'];
  for (const { value } of draft.arguments) {
    if (value.type === 'project-path') parts.push(value.path || '<project-path>');
    else if (value.type === 'literal') parts.push(value.value || '<fixed-argument>');
    else if (value.type === 'switch') parts.push(`[${value.value || `{${value.label || value.id}}`}]`);
    else {
      if (value.prefix) parts.push(value.prefix);
      parts.push(`{${value.label || value.id || 'Value'}}`);
    }
  }
  return parts.join(' ');
}
