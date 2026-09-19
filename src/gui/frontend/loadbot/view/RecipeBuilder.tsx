import { useState, type FormEvent } from 'react';
import type { LoadbotInterpreterRunner, LoadbotRecipeArgument } from '../contract';
import type { LoadbotActions, LoadbotState } from '../application/controller';
import { recipePreview, suggestedParameterId, type RecipeDraftArgument, type RecipeParameterKind } from '../application/recipeEditor';
import { Button, Checkbox, Dialog, InputControl, PathSelector, SelectControl, StatusDisplay, TextareaControl } from '../../ui/components';

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
        <fieldset className="lb-recipe-group"><legend>What should Loadbot do?</legend><div className="lb-choice-row">
          <Button aria-pressed={draft.recipe.behavior === 'run'} onClick={() => actions.setRecipeBehavior('run')} disabled={busy}>RUN RECIPE</Button>
          <Button aria-pressed={draft.recipe.behavior === 'launch'} onClick={() => actions.setRecipeBehavior('launch')} disabled={busy}>LAUNCH APPLICATION</Button>
        </div><p className="lb-note">Run will observe output later. Launch will hand the application to the OS. Execution is not available yet.</p></fieldset>
        <fieldset className="lb-recipe-group"><legend>What runs it?</legend>
          <SelectControl label="Run with" value={program.type} disabled={busy} onChange={(event) => {
            const type = event.target.value;
            actions.setRecipeProgram(type === 'project-file' ? { type, path: '' }
              : type === 'interpreter' ? { type, runner: 'python' }
                : { type: 'executable', name: '' });
          }}><option value="project-file">File in tool</option><option value="interpreter">Interpreter</option><option value="executable">Program</option></SelectControl>
          {program.type === 'project-file' && <PathSelector label="File" value={program.path} required disabled={busy}
            placeholder="scripts/tool.py" onChange={(event) => actions.setRecipeProgram({ ...program, path: event.target.value })}
            action={<Button disabled={busy} onClick={() => void actions.chooseRecipeProgramFile()}>BROWSE</Button>} />}
          {program.type === 'interpreter' && <SelectControl label="Runner" value={program.runner} disabled={busy}
            onChange={(event) => actions.setRecipeProgram({ type: 'interpreter', runner: event.target.value as LoadbotInterpreterRunner })}>
            <option value="bash">Bash</option><option value="sh">sh</option><option value="python">Python</option><option value="powershell">PowerShell</option>
          </SelectControl>}
          {program.type === 'executable' && <InputControl label="Program" value={program.name} required disabled={busy}
            placeholder="cargo" onChange={(event) => actions.setRecipeProgram({ ...program, name: event.target.value })} />}
        </fieldset>
        <fieldset className="lb-recipe-group"><legend>Run from</legend>
          <SelectControl label="Location" value={cwd.type} disabled={busy} onChange={(event) => {
            const type = event.target.value;
            actions.setRecipeWorkingDirectory(type === 'project-relative' ? { type, path: '' }
              : type === 'target-parent' ? { type } : { type: 'project-root' });
          }}><option value="project-root">Tool folder</option><option value="target-parent" disabled={program.type !== 'project-file'}>File's folder</option>
            <option value="project-relative">Folder inside tool</option></SelectControl>
          {cwd.type === 'project-relative' && <PathSelector label="Folder" value={cwd.path} required disabled={busy}
            placeholder="scripts/tools" onChange={(event) => actions.setRecipeWorkingDirectory({ ...cwd, path: event.target.value })}
            action={<Button disabled={busy} onClick={() => void actions.chooseRecipeWorkingDirectory()}>BROWSE</Button>} />}
        </fieldset>
        <fieldset className="lb-recipe-group"><legend>Options and inputs</legend>
          <p className="lb-note">Add each piece in the order Loadbot should pass it to the tool.</p>
          <div className="lb-parameter-add"><SelectControl label="Option type" value={parameterKind} disabled={busy}
            onChange={(event) => setParameterKind(event.target.value as RecipeParameterKind)}>
            <option value="text">Ask for text</option><option value="file">Ask for file</option><option value="directory">Ask for folder</option>
            <option value="switch">On/off flag</option><option value="prefixed-input">Flag + value</option><option value="literal">Fixed text</option><option value="project-path">File in tool</option>
          </SelectControl><Button onClick={() => actions.addRecipeParameter(parameterKind)} disabled={busy}>+ ADD OPTION</Button></div>
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
  const [advanced, setAdvanced] = useState(false);
  const value = argument.value;
  const label = value.type === 'project-path' ? 'FILE IN TOOL' : value.type === 'literal' ? 'FIXED TEXT'
    : value.type === 'switch' ? 'ON/OFF FLAG' : value.prefix !== undefined ? 'FLAG + VALUE' : value.kind === 'text' ? 'ASK FOR TEXT' : value.kind === 'file' ? 'ASK FOR FILE' : 'ASK FOR FOLDER';
  const update = (next: LoadbotRecipeArgument, idEdited?: boolean) => actions.updateRecipeParameter(argument.key, next, idEdited);
  return <article className="lb-parameter"><header><strong>{index + 1}. {label}</strong><span>
    <Button aria-label={`Move ${label} up`} disabled={busy || index === 0} onClick={() => actions.moveRecipeParameter(argument.key, -1)}>↑</Button>
    <Button aria-label={`Move ${label} down`} disabled={busy || index === count - 1} onClick={() => actions.moveRecipeParameter(argument.key, 1)}>↓</Button>
    <Button aria-label={`Remove ${label}`} disabled={busy} onClick={() => actions.removeRecipeParameter(argument.key)}>×</Button>
  </span></header>
    {value.type === 'project-path' && <PathSelector label="File" value={value.path} required disabled={busy} onChange={(event) => update({ ...value, path: event.target.value })}
      action={<Button disabled={busy} onClick={() => void actions.chooseRecipeArgumentPath(argument.key)}>BROWSE</Button>} />}
    {value.type === 'literal' && <InputControl label="Value" value={value.value} disabled={busy} onChange={(event) => update({ ...value, value: event.target.value })} />}
    {(value.type === 'input' || value.type === 'switch') && <>
      <InputControl label="Name" value={value.label} required disabled={busy} onChange={(event) => {
        const label = event.target.value;
        update({ ...value, label, id: argument.idManuallyEdited ? value.id : suggestedParameterId(label) });
      }} />
      <Button className="lb-advanced-toggle" aria-expanded={advanced} onClick={() => setAdvanced((open) => !open)}>{advanced ? 'HIDE ADVANCED' : 'ADVANCED'}</Button>
      {advanced && <InputControl label="Parameter ID" value={value.id} required disabled={busy} onChange={(event) => update({ ...value, id: event.target.value }, true)} />}
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
