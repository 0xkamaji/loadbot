// @vitest-environment node
import { describe, expect, it } from 'vitest';
import {
  addDraftArgument, draftValidation, moveDraftArgument, newRecipeDraft, recipeDraftFromShortcut,
  recipeFromDraft, recipePreview, removeDraftArgument, suggestedParameterId, updateDraftArgument,
  type RecipeDraft,
} from '../frontend/loadbot/application/recipeEditor';

describe('Recipe editor projection', () => {
  it('maps every user-facing parameter primitive onto ordered Phase 2 arguments', () => {
    let draft = newRecipeDraft('run');
    for (const [index, kind] of (['project-path', 'file', 'directory', 'text', 'switch', 'prefixed-input', 'literal'] as const).entries()) {
      draft = addDraftArgument(draft, kind, index + 1);
    }
    expect(recipeFromDraft(draft).arguments.map((argument) => argument.type)).toEqual([
      'project-path', 'input', 'input', 'input', 'switch', 'input', 'literal',
    ]);
    expect(recipeFromDraft(draft).arguments.slice(1, 4).map((argument) => argument.type === 'input' && argument.kind)).toEqual(['file', 'directory', 'text']);
    expect(recipeFromDraft(draft).arguments[5]).toMatchObject({ type: 'input', kind: 'text', prefix: '' });
  });

  it('reorders/removes by stable UI key without changing semantic parameter IDs', () => {
    let draft = addDraftArgument(addDraftArgument(newRecipeDraft('launch'), 'text', 10), 'switch', 20);
    const first = draft.arguments[0]!;
    draft = updateDraftArgument(draft, first.key, { type: 'input', id: 'sample', label: 'Sample', kind: 'text', required: true }, true);
    draft = moveDraftArgument(draft, first.key, 1);
    expect(draft.arguments.map((argument) => argument.key)).toEqual([20, 10]);
    expect(draft.arguments[1]).toMatchObject({ key: 10, value: { id: 'sample' }, idManuallyEdited: true });
    draft = removeDraftArgument(draft, 20);
    expect(draft.arguments.map((argument) => argument.key)).toEqual([10]);
  });

  it('suggests portable IDs, validates duplicate IDs, and renders preview placeholders only', () => {
    expect(suggestedParameterId('Input Sample')).toBe('input-sample');
    let draft: RecipeDraft = { ...newRecipeDraft('run'), name: 'triage', recipe: {
      ...newRecipeDraft('run').recipe, program: { type: 'interpreter' as const, runner: 'python' as const },
    } };
    draft = addDraftArgument(addDraftArgument(draft, 'text', 1), 'prefixed-input', 2);
    draft = updateDraftArgument(draft, 1, { type: 'input', id: 'format', label: 'Format', kind: 'text', required: true }, true);
    draft = updateDraftArgument(draft, 2, { type: 'input', id: 'format', label: 'Output Format', kind: 'text', required: false, prefix: '--format' }, true);
    expect(draftValidation(draft)).toContain('Duplicate parameter ID: format');
    expect(recipePreview(draft)).toBe('python {Format} --format {Output Format}');
  });

  it('reopens authoritative personal Recipes and refuses Legacy/shared editing projections', () => {
    const recipe = { version: 1, behavior: 'launch' as const, program: { type: 'executable' as const, name: 'viewer' },
      working_directory: { type: 'project-root' as const }, arguments: [{ type: 'literal' as const, value: 'one value' }] };
    expect(recipeDraftFromShortcut({ name: 'view', source: 'personal', description: 'Open it', recipe })).toMatchObject({
      mode: 'edit', name: 'view', description: 'Open it', recipe: { behavior: 'launch' }, arguments: [{ value: { value: 'one value' } }],
    });
    expect(recipeDraftFromShortcut({ name: 'old', source: 'personal', path: 'old.sh' })).toBeUndefined();
    expect(recipeDraftFromShortcut({ name: 'shared', source: 'catalog', recipe })).toBeUndefined();
  });
});
