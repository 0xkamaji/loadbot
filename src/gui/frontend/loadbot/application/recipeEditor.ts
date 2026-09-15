import type { LoadbotRecipe, LoadbotRecipeArgument, LoadbotShortcut } from '../contract';

export type RecipeParameterKind = 'project-path' | 'text' | 'file' | 'directory' | 'switch' | 'prefixed-input' | 'literal';
export interface RecipeDraftArgument {
  readonly key: number;
  readonly value: LoadbotRecipeArgument;
  readonly idManuallyEdited: boolean;
}
export interface RecipeDraft {
  readonly mode: 'create' | 'edit';
  readonly name: string;
  readonly description: string;
  readonly recipe: Omit<LoadbotRecipe, 'arguments'>;
  readonly arguments: readonly RecipeDraftArgument[];
}

export const suggestedParameterId = (label: string) => label.trim().toLowerCase()
  .replace(/[^a-z0-9_.-]+/g, '-').replace(/^-+|-+$/g, '') || 'parameter';

export function newRecipeDraft(behavior: LoadbotRecipe['behavior']): RecipeDraft {
  return {
    mode: 'create', name: '', description: '',
    recipe: {
      version: 1, behavior,
      program: { type: 'executable', name: '' },
      working_directory: { type: 'project-root' },
    },
    arguments: [],
  };
}

export function recipeDraftFromShortcut(shortcut: LoadbotShortcut): RecipeDraft | undefined {
  if (!shortcut.recipe || shortcut.source !== 'personal') return undefined;
  return {
    mode: 'edit', name: shortcut.name, description: shortcut.description ?? '',
    recipe: { ...shortcut.recipe, program: { ...shortcut.recipe.program }, working_directory: { ...shortcut.recipe.working_directory } },
    arguments: shortcut.recipe.arguments.map((value, key) => ({ key, value: { ...value }, idManuallyEdited: true })),
  };
}

export function addDraftArgument(draft: RecipeDraft, kind: RecipeParameterKind, key: number): RecipeDraft {
  const value: LoadbotRecipeArgument = kind === 'project-path' ? { type: 'project-path', path: '' }
    : kind === 'literal' ? { type: 'literal', value: '' }
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
  return { ...draft.recipe, arguments: draft.arguments.map((argument) => argument.value) };
}

export function draftValidation(draft: RecipeDraft): readonly string[] {
  const errors: string[] = [];
  if (!draft.name.trim()) errors.push('Shortcut name is required.');
  const program = draft.recipe.program;
  if (program.type === 'project-file' && !program.path.trim()) errors.push('Project file is required.');
  if (program.type === 'executable' && !program.name.trim()) errors.push('Executable is required.');
  if (draft.recipe.working_directory.type === 'project-relative' && !draft.recipe.working_directory.path.trim()) errors.push('Working directory is required.');
  if (draft.recipe.working_directory.type === 'target-parent' && program.type !== 'project-file') errors.push('Target parent requires a Project File program.');
  const ids = new Set<string>();
  for (const argument of draft.arguments) {
    const value = argument.value;
    if (value.type === 'project-path' && !value.path.trim()) errors.push('Fixed project path is required.');
    if (value.type === 'input' || value.type === 'switch') {
      if (!value.label.trim()) errors.push('Parameter label is required.');
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
  const program = draft.recipe.program.type === 'project-file' ? draft.recipe.program.path || '<project-file>'
    : draft.recipe.program.type === 'interpreter' ? draft.recipe.program.runner
      : draft.recipe.program.name || '<executable>';
  const parts = [program];
  for (const { value } of draft.arguments) {
    if (value.type === 'project-path') parts.push(value.path || '<project-path>');
    else if (value.type === 'literal') parts.push(value.value || '<literal>');
    else if (value.type === 'switch') parts.push(`[${value.value || `{${value.label || value.id}}`}]`);
    else {
      if (value.prefix) parts.push(value.prefix);
      parts.push(`{${value.label || value.id || 'Value'}}`);
    }
  }
  return parts.join(' ');
}
