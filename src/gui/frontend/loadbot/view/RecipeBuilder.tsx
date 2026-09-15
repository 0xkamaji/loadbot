import { useState, type FormEvent } from 'react';
import type { LoadbotInterpreterRunner, LoadbotRecipeArgument } from '../contract';
import type { LoadbotActions, LoadbotState } from '../application/controller';
import { recipePreview, suggestedParameterId, type RecipeDraftArgument, type RecipeParameterKind } from '../application/recipeEditor';
import { Button, Checkbox, Dialog, InputControl, SelectControl, StatusDisplay, TextareaControl } from '../../ui/components';

export function RecipeBuilder({ state, actions, onClose, onDone }: {
  state: LoadbotState; actions: LoadbotActions; onClose(): void; onDone(): void;
}) {
  const editor = state.recipeEditor;
  const [parameterKind, setParameterKind] = useState<RecipeParameterKind>('text');
  if (!editor) return null;
  const { draft, errors } = editor;
  const busy = state.management.status === 'submitting';
  const program = draft.recipe.program;
  const cwd = draft.recipe.working_directory;
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (await actions.saveRecipe()) onDone();
  }
  return <Dialog label={draft.mode === 'create' ? 'Create Recipe shortcut' : 'Edit Recipe shortcut'} className="lb-recipe-dialog" onClose={onClose}>
    <h2>{draft.mode === 'create' ? 'CREATE RECIPE' : `EDIT RECIPE / ${draft.name}`}</h2>
    <form onSubmit={submit}>
      <div className="lb-recipe-grid">
        <InputControl autoFocus label="Shortcut name" value={draft.name}
          onChange={(event) => actions.updateRecipeDetails({ name: event.target.value })} required disabled={busy || draft.mode === 'edit'} />
        <TextareaControl label="Description (optional)" value={draft.description}
          onChange={(event) => actions.updateRecipeDetails({ description: event.target.value })} disabled={busy} />
        <fieldset className="lb-recipe-group"><legend>Behavior</legend><div className="lb-choice-row">
          <Button aria-pressed={draft.recipe.behavior === 'run'} onClick={() => actions.setRecipeBehavior('run')} disabled={busy}>RUN RECIPE</Button>
          <Button aria-pressed={draft.recipe.behavior === 'launch'} onClick={() => actions.setRecipeBehavior('launch')} disabled={busy}>LAUNCH APPLICATION</Button>
        </div><p className="lb-note">Defines future lifecycle behavior only. Execution is not available.</p></fieldset>
        <fieldset className="lb-recipe-group"><legend>Program</legend>
          <SelectControl label="Program type" value={program.type} disabled={busy} onChange={(event) => {
            const type = event.target.value;
            actions.setRecipeProgram(type === 'project-file' ? { type, path: '' }
              : type === 'interpreter' ? { type, runner: 'python' }
                : { type: 'executable', name: '' });
          }}><option value="project-file">Project File</option><option value="interpreter">Interpreter</option><option value="executable">Executable</option></SelectControl>
          {program.type === 'project-file' && <InputControl label="Project-relative target" value={program.path} required disabled={busy}
            placeholder="scripts/tool.py" onChange={(event) => actions.setRecipeProgram({ ...program, path: event.target.value })} />}
          {program.type === 'interpreter' && <SelectControl label="Runner" value={program.runner} disabled={busy}
            onChange={(event) => actions.setRecipeProgram({ type: 'interpreter', runner: event.target.value as LoadbotInterpreterRunner })}>
            <option value="bash">Bash</option><option value="sh">sh</option><option value="python">Python</option><option value="powershell">PowerShell</option>
          </SelectControl>}
          {program.type === 'executable' && <InputControl label="Executable" value={program.name} required disabled={busy}
            placeholder="cargo" onChange={(event) => actions.setRecipeProgram({ ...program, name: event.target.value })} />}
        </fieldset>
        <fieldset className="lb-recipe-group"><legend>Working directory</legend>
          <SelectControl label="Working directory" value={cwd.type} disabled={busy} onChange={(event) => {
            const type = event.target.value;
            actions.setRecipeWorkingDirectory(type === 'project-relative' ? { type, path: '' }
              : type === 'target-parent' ? { type } : { type: 'project-root' });
          }}><option value="project-root">Project root</option><option value="target-parent" disabled={program.type !== 'project-file'}>Target parent</option>
            <option value="project-relative">Project-relative directory</option></SelectControl>
          {cwd.type === 'project-relative' && <InputControl label="Project-relative directory" value={cwd.path} required disabled={busy}
            placeholder="scripts/tools" onChange={(event) => actions.setRecipeWorkingDirectory({ ...cwd, path: event.target.value })} />}
        </fieldset>
        <fieldset className="lb-recipe-group"><legend>Ordered parameters</legend>
          <div className="lb-parameter-add"><SelectControl label="Parameter type" value={parameterKind} disabled={busy}
            onChange={(event) => setParameterKind(event.target.value as RecipeParameterKind)}>
            <option value="project-path">Fixed Project Path</option><option value="file">Runtime File</option><option value="directory">Runtime Directory</option>
            <option value="text">Value</option><option value="switch">Flag</option><option value="prefixed-input">Flag + Value</option><option value="literal">Literal</option>
          </SelectControl><Button onClick={() => actions.addRecipeParameter(parameterKind)} disabled={busy}>+ ADD PARAMETER</Button></div>
          <div className="lb-parameter-list">{draft.arguments.map((argument, index) => <ParameterEditor key={argument.key}
            argument={argument} index={index} count={draft.arguments.length} busy={busy} actions={actions} />)}
            {!draft.arguments.length && <p className="lb-note">No arguments. The program will receive an empty argument vector.</p>}
          </div>
        </fieldset>
        <section className="lb-recipe-preview" aria-label="Recipe preview"><h3>PREVIEW</h3><code>{recipePreview(draft)}</code>
          <p>{draft.recipe.behavior === 'run' ? 'Run Recipe' : 'Launch Application'} · preview only</p></section>
        {!!errors.length && <div className="lb-recipe-errors" role="alert">{errors.map((error) => <p key={error}>{error}</p>)}</div>}
        {state.management.status !== 'idle' && (state.management.kind === 'add-shortcut' || state.management.kind === 'update-shortcut')
          && <StatusDisplay>{state.management.message}</StatusDisplay>}
      </div>
      <div className="lb-dialog-actions"><Button onClick={onClose} disabled={busy}>CANCEL</Button>
        <Button type="submit" disabled={busy}>{busy ? 'SAVING…' : draft.mode === 'create' ? 'CREATE RECIPE' : 'SAVE RECIPE'}</Button></div>
    </form>
  </Dialog>;
}

function ParameterEditor({ argument, index, count, busy, actions }: {
  argument: RecipeDraftArgument; index: number; count: number; busy: boolean; actions: LoadbotActions;
}) {
  const value = argument.value;
  const label = value.type === 'project-path' ? 'FIXED PROJECT PATH' : value.type === 'literal' ? 'LITERAL'
    : value.type === 'switch' ? 'FLAG' : value.prefix !== undefined ? 'FLAG + VALUE' : value.kind === 'text' ? 'VALUE' : `RUNTIME ${value.kind.toUpperCase()}`;
  const update = (next: LoadbotRecipeArgument, idEdited?: boolean) => actions.updateRecipeParameter(argument.key, next, idEdited);
  return <article className="lb-parameter"><header><strong>{index + 1}. {label}</strong><span>
    <Button aria-label={`Move ${label} up`} disabled={busy || index === 0} onClick={() => actions.moveRecipeParameter(argument.key, -1)}>↑</Button>
    <Button aria-label={`Move ${label} down`} disabled={busy || index === count - 1} onClick={() => actions.moveRecipeParameter(argument.key, 1)}>↓</Button>
    <Button aria-label={`Remove ${label}`} disabled={busy} onClick={() => actions.removeRecipeParameter(argument.key)}>×</Button>
  </span></header>
    {value.type === 'project-path' && <InputControl label="Project-relative path" value={value.path} required disabled={busy} onChange={(event) => update({ ...value, path: event.target.value })} />}
    {value.type === 'literal' && <InputControl label="Value" value={value.value} disabled={busy} onChange={(event) => update({ ...value, value: event.target.value })} />}
    {(value.type === 'input' || value.type === 'switch') && <>
      <InputControl label="Label" value={value.label} required disabled={busy} onChange={(event) => {
        const label = event.target.value;
        update({ ...value, label, id: argument.idManuallyEdited ? value.id : suggestedParameterId(label) });
      }} />
      <InputControl label="ID" value={value.id} required disabled={busy} onChange={(event) => update({ ...value, id: event.target.value }, true)} />
    </>}
    {value.type === 'switch' && <><InputControl label="Argument" value={value.value} required disabled={busy} placeholder="--recursive" onChange={(event) => update({ ...value, value: event.target.value })} />
      <Checkbox label="Default enabled" checked={value.default} disabled={busy} onChange={(event) => update({ ...value, default: event.target.checked })} /></>}
    {value.type === 'input' && <>
      {value.prefix !== undefined && <InputControl label="Flag" value={value.prefix} required disabled={busy} placeholder="--format" onChange={(event) => update({ ...value, prefix: event.target.value })} />}
      <Checkbox label="Required" checked={value.required} disabled={busy} onChange={(event) => update({ ...value, required: event.target.checked })} />
      <InputControl label="Default (optional)" value={value.default ?? ''} disabled={busy}
        onChange={(event) => update({ ...value, default: event.target.value || undefined })} />
    </>}
  </article>;
}
