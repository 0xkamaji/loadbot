import { useState, type FormEvent } from 'react';
import type { LoadbotRecipeArgument, LoadbotRunner } from '../contract';
import type { LoadbotActions, LoadbotState } from '../application/controller';
import { recipePreview, suggestedParameterId, type RecipeDraftArgument, type RecipeParameterKind } from '../application/recipeEditor';
import { Button, Checkbox, Dialog, InputControl, PathSelector, SelectControl, StatusDisplay, TextareaControl } from '../../ui/components';

export function RecipeBuilder({ state, actions, onClose, onDone }: {
  state: LoadbotState; actions: LoadbotActions; onClose(): void; onDone(): void;
}) {
  const editor = state.recipeEditor;
  const [parameterKind, setParameterKind] = useState<RecipeParameterKind>('text');
  const [advanced, setAdvanced] = useState(false);
  if (!editor) return null;
  const { draft, errors } = editor;
  const busy = state.management.status === 'submitting';
  const cwd = draft.workingDirectory;
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (await actions.saveRecipe()) onDone();
  }
  return <Dialog label={draft.mode === 'create' ? 'Create shortcut' : 'Edit shortcut'} className="lb-recipe-dialog" onClose={onClose}>
    <h2>{draft.mode === 'create' ? 'CREATE SHORTCUT' : `EDIT SHORTCUT / ${draft.name}`}</h2>
    <form onSubmit={submit}>
      <div className="lb-recipe-grid">
        <InputControl autoFocus label="Name" value={draft.name}
          onChange={(event) => actions.updateRecipeDetails({ name: event.target.value })} required disabled={busy || draft.mode === 'edit'} />
        <PathSelector label="Target" value={draft.target} required disabled={busy} placeholder="scripts/tool.py"
          onChange={(event) => actions.setRecipeTarget(event.target.value)}
          action={<Button disabled={busy} onClick={() => void actions.chooseRecipeTarget()}>BROWSE</Button>} />
        <SelectControl label="Run with" value={draft.runner} disabled={busy}
          onChange={(event) => actions.setRecipeRunner(event.target.value as LoadbotRunner)}>
          <option value="direct">Direct / Program</option><option value="python">Python</option>
          <option value="powershell">PowerShell</option><option value="bash">Bash</option><option value="sh">Shell</option>
        </SelectControl>
        <div className="lb-help-action"><Button disabled={busy || editor.help?.status === 'loading'}
          onClick={() => void actions.viewRecipeHelp()}>{editor.help?.status === 'loading' ? 'LOADING HELP…' : 'VIEW HELP'}</Button></div>
        {editor.help && <HelpPanel help={editor.help} onDismiss={actions.dismissRecipeHelp} />}
        <TextareaControl label="Description (optional)" value={draft.description}
          onChange={(event) => actions.updateRecipeDetails({ description: event.target.value })} disabled={busy} />

        <fieldset className="lb-recipe-group"><legend>Parameters</legend>
          <p className="lb-note">Parameters are passed to the target in this order.</p>
          <div className="lb-parameter-add"><SelectControl label="Parameter type" value={parameterKind} disabled={busy}
            onChange={(event) => setParameterKind(event.target.value as RecipeParameterKind)}>
            <option value="text">Value</option><option value="file">File</option><option value="directory">Directory</option>
            <option value="switch">Flag</option><option value="prefixed-input">Flag + Value</option><option value="literal">Fixed Argument</option>
          </SelectControl><Button onClick={() => actions.addRecipeParameter(parameterKind)} disabled={busy}>+ ADD PARAMETER</Button></div>
          <div className="lb-parameter-list">{draft.arguments.map((argument, index) => <ParameterEditor key={argument.key}
            argument={argument} index={index} count={draft.arguments.length} busy={busy} actions={actions} />)}
            {!draft.arguments.length && <p className="lb-note">No parameters.</p>}
          </div>
        </fieldset>

        <section className="lb-recipe-advanced">
          <Button className="lb-advanced-toggle" aria-expanded={advanced} onClick={() => setAdvanced((open) => !open)}>
            {advanced ? 'HIDE ADVANCED' : 'ADVANCED'}
          </Button>
          {advanced && <fieldset className="lb-recipe-group"><legend>Run from</legend>
            <SelectControl label="Location" value={cwd.type} disabled={busy} onChange={(event) => {
              const type = event.target.value;
              actions.setRecipeWorkingDirectory(type === 'project-relative' ? { type, path: '' }
                : type === 'target-parent' ? { type } : { type: 'project-root' });
            }}><option value="project-root">Tool folder</option><option value="target-parent" disabled={draft.runner !== 'direct'}>Target's folder</option>
              <option value="project-relative">Folder inside tool</option></SelectControl>
            {cwd.type === 'project-relative' && <PathSelector label="Folder" value={cwd.path} required disabled={busy}
              placeholder="scripts/tools" onChange={(event) => actions.setRecipeWorkingDirectory({ ...cwd, path: event.target.value })
              } action={<Button disabled={busy} onClick={() => void actions.chooseRecipeWorkingDirectory()}>BROWSE</Button>} />}
          </fieldset>}
        </section>

        <section className="lb-recipe-preview" aria-label="Shortcut preview"><h3>PREVIEW</h3><code>{recipePreview(draft)}</code>
          <p>Preview only</p></section>
        {!!errors.length && <div className="lb-recipe-errors" role="alert">{errors.map((error) => <p key={error}>{error}</p>)}</div>}
        {state.management.status !== 'idle' && (state.management.kind === 'add-shortcut' || state.management.kind === 'update-shortcut')
          && <StatusDisplay>{state.management.message}</StatusDisplay>}
      </div>
      <div className="lb-dialog-actions"><Button onClick={onClose} disabled={busy}>CANCEL</Button>
        <Button type="submit" disabled={busy}>{busy ? 'SAVING…' : draft.mode === 'create' ? 'CREATE SHORTCUT' : 'SAVE SHORTCUT'}</Button></div>
    </form>
  </Dialog>;
}

function HelpPanel({ help, onDismiss }: {
  help: NonNullable<LoadbotState['recipeEditor']>['help']; onDismiss(): void;
}) {
  if (!help) return null;
  const empty = help.status === 'ready' && !help.result.stdout.trim() && !help.result.stderr.trim();
  return <section className="lb-help-panel" aria-label="Target help">
    <header><h3>HELP</h3><Button onClick={onDismiss}>HIDE</Button></header>
    {help.status === 'loading' && <p>Requesting help from the selected target…</p>}
    {help.status === 'error' && <p role="alert">{help.message}</p>}
    {help.status === 'ready' && <>
      <p className="lb-note">{help.result.detectedHelpFlag
        ? `${help.result.detectedHelpFlag} · exit ${help.result.exitStatus ?? 'unknown'}`
        : `No help output · exit ${help.result.exitStatus ?? 'unknown'}`}</p>
      {empty && <p>No help output was returned for --help or -h.</p>}
      {!empty && <div className="lb-help-output">
        {!!help.result.stdout.trim() && <section><h4>STDOUT</h4><pre>{help.result.stdout}</pre></section>}
        {!!help.result.stderr.trim() && <section><h4>STDERR</h4><pre>{help.result.stderr}</pre></section>}
      </div>}
    </>}
  </section>;
}

function ParameterEditor({ argument, index, count, busy, actions }: {
  argument: RecipeDraftArgument; index: number; count: number; busy: boolean; actions: LoadbotActions;
}) {
  const [advanced, setAdvanced] = useState(false);
  const value = argument.value;
  const label = value.type === 'project-path' ? 'FILE IN TOOL' : value.type === 'literal' ? 'FIXED ARGUMENT'
    : value.type === 'switch' ? 'FLAG' : value.prefix !== undefined ? 'FLAG + VALUE' : value.kind === 'text' ? 'VALUE' : value.kind === 'file' ? 'FILE' : 'DIRECTORY';
  const update = (next: LoadbotRecipeArgument, idEdited?: boolean) => actions.updateRecipeParameter(argument.key, next, idEdited);
  return <article className="lb-parameter"><header><strong>{index + 1}. {label}</strong><span>
    <Button aria-label={`Move ${label} up`} disabled={busy || index === 0} onClick={() => actions.moveRecipeParameter(argument.key, -1)}>↑</Button>
    <Button aria-label={`Move ${label} down`} disabled={busy || index === count - 1} onClick={() => actions.moveRecipeParameter(argument.key, 1)}>↓</Button>
    <Button aria-label={`Remove ${label}`} disabled={busy} onClick={() => actions.removeRecipeParameter(argument.key)}>×</Button>
  </span></header>
    {value.type === 'project-path' && <PathSelector label="File" value={value.path} required disabled={busy} onChange={(event) => update({ ...value, path: event.target.value })
      } action={<Button disabled={busy} onClick={() => void actions.chooseRecipeArgumentPath(argument.key)}>BROWSE</Button>} />}
    {value.type === 'literal' && <InputControl label="Argument" value={value.value} disabled={busy} onChange={(event) => update({ ...value, value: event.target.value })} />}
    {(value.type === 'input' || value.type === 'switch') && <>
      <InputControl label="Name" value={value.label} required disabled={busy} onChange={(event) => {
        const label = event.target.value;
        update({ ...value, label, id: argument.idManuallyEdited ? value.id : suggestedParameterId(label) });
      }} />
      <Button className="lb-advanced-toggle" aria-expanded={advanced} onClick={() => setAdvanced((open) => !open)}>{advanced ? 'HIDE ADVANCED' : 'ADVANCED'}</Button>
      {advanced && <InputControl label="Parameter ID" value={value.id} required disabled={busy} onChange={(event) => update({ ...value, id: event.target.value }, true)} />}
    </>}
    {value.type === 'switch' && <><InputControl label="Flag" value={value.value} required disabled={busy} placeholder="--recursive" onChange={(event) => update({ ...value, value: event.target.value })} />
      <Checkbox label="Default enabled" checked={value.default} disabled={busy} onChange={(event) => update({ ...value, default: event.target.checked })} /></>}
    {value.type === 'input' && <>
      {value.prefix !== undefined && <InputControl label="Flag" value={value.prefix} required disabled={busy} placeholder="--format" onChange={(event) => update({ ...value, prefix: event.target.value })} />}
      <Checkbox label="Required" checked={value.required} disabled={busy} onChange={(event) => update({ ...value, required: event.target.checked })} />
      <InputControl label="Default (optional)" value={value.default ?? ''} disabled={busy}
        onChange={(event) => update({ ...value, default: event.target.value || undefined })} />
    </>}
  </article>;
}
