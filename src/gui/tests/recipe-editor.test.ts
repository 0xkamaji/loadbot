// @vitest-environment node
import { describe, expect, it } from 'vitest';
import {
  addDraftArgument, draftValidation, moveDraftArgument, newRecipeDraft, recipeDraftFromShortcut,
  recipeFromDraft, recipePreview, removeDraftArgument, suggestedParameterId, suggestedRunner,
  updateDraftArgument, updateDraftRunner, updateDraftTarget, type RecipeDraft,
} from '../frontend/loadbot/application/recipeEditor';

describe('simplified shortcut editor projection', () => {
  it('maps target, runner, and every parameter onto the unchanged Phase 2 Recipe model', () => {
    let draft = updateDraftRunner(updateDraftTarget(newRecipeDraft(), 'scripts/triage.py'), 'python');
    for (const [index, kind] of (['file', 'directory', 'text', 'switch', 'prefixed-input', 'literal'] as const).entries()) {
      draft = addDraftArgument(draft, kind, index + 1);
    }
    const recipe = recipeFromDraft(draft);
    expect(recipe).toMatchObject({
      version: 1, behavior: 'run', program: { type: 'interpreter', runner: 'python' },
      working_directory: { type: 'project-root' },
    });
    expect(recipe.arguments.map((argument) => argument.type)).toEqual([
      'project-path', 'input', 'input', 'input', 'switch', 'input', 'literal',
    ]);
    expect(recipe.arguments.slice(1, 4).map((argument) => argument.type === 'input' && argument.kind)).toEqual(['file', 'directory', 'text']);

    const direct = recipeFromDraft(updateDraftRunner(draft, 'direct'));
    expect(direct.program).toEqual({ type: 'project-file', path: 'scripts/triage.py' });
    expect(direct.arguments[0]?.type).toBe('input');
  });

  it('suggests runners deterministically until the user overrides one', () => {
    expect(suggestedRunner('tool.PY')).toBe('python');
    expect(suggestedRunner('setup.ps1')).toBe('powershell');
    expect(suggestedRunner('scripts/run.sh')).toBe('bash');
    expect(suggestedRunner('bin/tool')).toBe('direct');
    let draft = updateDraftTarget(newRecipeDraft(), 'one.py');
    expect(draft.runner).toBe('python');
    draft = updateDraftRunner(draft, 'bash');
    expect(updateDraftTarget(draft, 'two.ps1').runner).toBe('bash');
  });

  it('reorders/removes by stable UI key without changing semantic parameter IDs', () => {
    let draft = addDraftArgument(addDraftArgument(newRecipeDraft(), 'text', 10), 'switch', 20);
    const first = draft.arguments[0]!;
    draft = updateDraftArgument(draft, first.key, { type: 'input', id: 'sample', label: 'Sample', kind: 'text', required: true }, true);
    draft = moveDraftArgument(draft, first.key, 1);
    expect(draft.arguments.map((argument) => argument.key)).toEqual([20, 10]);
    expect(draft.arguments[1]).toMatchObject({ key: 10, value: { id: 'sample' }, idManuallyEdited: true });
    draft = removeDraftArgument(draft, 20);
    expect(draft.arguments.map((argument) => argument.key)).toEqual([10]);
  });

  it('validates IDs and renders a presentation-only target-first preview', () => {
    expect(suggestedParameterId('Input Sample')).toBe('input-sample');
    let draft: RecipeDraft = { ...updateDraftTarget(newRecipeDraft(), 'triage.py'), name: 'triage' };
    draft = addDraftArgument(addDraftArgument(draft, 'text', 1), 'prefixed-input', 2);
    draft = updateDraftArgument(draft, 1, { type: 'input', id: 'format', label: 'Format', kind: 'text', required: true }, true);
    draft = updateDraftArgument(draft, 2, { type: 'input', id: 'format', label: 'Output Format', kind: 'text', required: false, prefix: '--format' }, true);
    expect(draftValidation(draft)).toContain('Duplicate parameter ID: format');
    expect(recipePreview(draft)).toBe('python triage.py {Format} --format {Output Format}');
  });

  it('round-trips supported Recipes and refuses lossy Launch, Executable, Legacy, and shared edits', () => {
    const supported = { version: 1, behavior: 'run' as const, program: { type: 'interpreter' as const, runner: 'python' as const },
      working_directory: { type: 'project-root' as const }, arguments: [
        { type: 'project-path' as const, path: 'scripts/tool.py' }, { type: 'literal' as const, value: 'one value' },
      ] };
    const draft = recipeDraftFromShortcut({ name: 'tool', source: 'personal', description: 'Run it', recipe: supported });
    expect(draft).toMatchObject({ mode: 'edit', target: 'scripts/tool.py', runner: 'python', arguments: [{ value: { value: 'one value' } }] });
    expect(recipeFromDraft(draft!)).toEqual(supported);

    const launch = { ...supported, behavior: 'launch' as const };
    const executable = { ...supported, program: { type: 'executable' as const, name: 'cargo' }, arguments: [] };
    expect(recipeDraftFromShortcut({ name: 'launch', source: 'personal', recipe: launch })).toBeUndefined();
    expect(recipeDraftFromShortcut({ name: 'advanced', source: 'personal', recipe: executable })).toBeUndefined();
    expect(recipeDraftFromShortcut({ name: 'old', source: 'personal', path: 'old.sh' })).toBeUndefined();
    expect(recipeDraftFromShortcut({ name: 'shared', source: 'catalog', recipe: supported })).toBeUndefined();
  });
});
