import { useId } from 'react';
import type { LoadbotActions, LoadbotState } from '../application/controller';
import { recipeDraftFromShortcut } from '../application/recipeEditor';
import { Button, Checkbox, InputControl, PathSelector, StatusDisplay } from '../../ui/components';

/** Chooses widgets and wording. Application state supplies values and validation. */
export function ShortcutDetails({ state, actions, mode }: { state: LoadbotState; actions: LoadbotActions; mode: 'local' | 'fixture' }) {
  const statusId = useId();
  const { project, shortcut, fields, values, missingInputIds } = state;
  if (!shortcut) return <div className="lb-details"><p>Select a shortcut to see its details.</p></div>;
  const editableDraft = recipeDraftFromShortcut(shortcut);
  if (mode === 'local' || shortcut.recipe) return <div className="lb-details">
    <h3>{shortcut.name}</h3>
    {shortcut.description && <p>{shortcut.description}</p>}
    <dl className="lb-facts">
      <dt>Project</dt><dd>{project?.tool}</dd>
      <dt>Catalog</dt><dd>{project?.catalog}</dd>
      <dt>Source</dt><dd>{shortcut.source === 'catalog' ? 'Shared catalog command' : 'Personal shortcut'}</dd>
      {shortcut.path && <><dt>Runner</dt><dd>{shortcut.runner ?? 'Not specified'}</dd>
        <dt>Target</dt><dd title={shortcut.path}>{shortcut.path}</dd></>}
      {shortcut.recipe && <><dt>Invocation</dt><dd>Recipe version {shortcut.recipe.version}</dd>
        <dt>Behavior</dt><dd>{shortcut.recipe.behavior === 'run' ? 'Run Recipe' : 'Launch Application'}</dd>
        {editableDraft && <><dt>Target</dt><dd title={editableDraft.target}>{editableDraft.target}</dd>
          <dt>Run with</dt><dd>{editableDraft.runner === 'direct' ? 'Direct / Program' : editableDraft.runner}</dd></>}
        {!editableDraft && <><dt>Runs with</dt><dd>{shortcut.recipe.program.type === 'project-file' ? `File in tool · ${shortcut.recipe.program.path}`
          : shortcut.recipe.program.type === 'interpreter' ? `Interpreter · ${shortcut.recipe.program.runner}`
            : `Program · ${shortcut.recipe.program.name}`}</dd></>}
        <dt>Runs from</dt><dd>{shortcut.recipe.working_directory.type === 'project-relative'
          ? `Folder inside tool · ${shortcut.recipe.working_directory.path}` : shortcut.recipe.working_directory.type === 'target-parent' ? "File's folder" : 'Tool folder'}</dd>
        <dt>Parameters</dt><dd>{(editableDraft?.arguments.length ?? shortcut.recipe.arguments.length) || 'None'}</dd></>}
    </dl>
    {mode === 'local' && shortcut.source === 'personal' && <>
      <div className="lb-shortcut-actions">
        <Button className="lb-edit-recipe" disabled={!editableDraft}
          title={!editableDraft ? 'This shortcut is preserved but cannot be safely edited in the simplified editor.' : undefined}
          onClick={() => actions.openSelectedRecipeEditor()}>EDIT</Button>
        <Button className="lb-danger-action" onClick={actions.requestCurrentShortcutDeletion}>DELETE</Button>
      </div>
      {!editableDraft && <p className="lb-note">This {shortcut.recipe?.behavior === 'launch' ? 'Launch shortcut' : shortcut.recipe ? 'advanced Recipe' : 'Legacy shortcut'} is preserved and read-only in the simplified editor.</p>}
    </>}
    {shortcut.source === 'catalog' && <p className="lb-note">Shared catalog shortcuts are read-only here.</p>}
    <p className="lb-note">Inventory details. Execution is not connected.</p>
  </div>;
  const missing = fields.filter((field) => missingInputIds.includes(field.id));
  return <div className="lb-details">
    <h3>{shortcut.name}</h3>
    {shortcut.description && <p>{shortcut.description}</p>}
    <p className="lb-metadata" title={shortcut.path}>{shortcut.source === 'catalog' ? 'Shared' : 'Personal'} · {shortcut.runner ?? (mode === 'fixture' ? 'Inferred runner' : 'Runner not specified')} · {shortcut.path}</p>
    <form onSubmit={(event) => event.preventDefault()} aria-label={mode === 'fixture' ? 'Sample shortcut inputs' : 'Shortcut details'}>
      <div className="lb-fields">
        {fields.map((field) => field.kind === 'boolean'
          ? <Checkbox key={field.id} label={field.label} checked={Boolean(values[field.id])} onChange={(event) => actions.changeSampleInput(field.id, event.target.checked)} />
          : field.kind === 'path'
            ? <PathSelector key={field.id} label={field.label} value={String(values[field.id] ?? '')}
              onChange={(event) => actions.changeSampleInput(field.id, event.target.value)} required={field.required}
              aria-invalid={missingInputIds.includes(field.id) ? true : undefined} aria-describedby={statusId}
              placeholder={`Choose a sample ${field.pathKind}…`} title={String(values[field.id] ?? '')} spellCheck={false}
              action={<Button onClick={() => actions.useSamplePath(field.id)} aria-label={`Use sample ${field.label.toLowerCase()}`}
                title={`Set the sample value “${field.sampleValue}”; no filesystem access`}>
                {values[field.id] ? 'Change sample' : `Sample ${field.pathKind}…`}
              </Button>} />
            : <InputControl key={field.id} label={field.label} value={String(values[field.id] ?? '')} required={field.required} placeholder={field.placeholder}
              aria-invalid={missingInputIds.includes(field.id) ? true : undefined} aria-describedby={statusId}
              onChange={(event) => actions.changeSampleInput(field.id, event.target.value)} />)}
        {!fields.length && <p className="lb-note">No sample inputs for this shortcut.</p>}
      </div>
      <div className="lb-run">
        <Button disabled aria-describedby={statusId}>RUN SHORTCUT</Button>
        <StatusDisplay id={statusId}>{missing.length
          ? `Input required: ${missing.map((field) => field.label.toLowerCase()).join(', ')}. Execution is not connected.`
          : 'Sample form ready. Execution is not connected.'}</StatusDisplay>
      </div>
    </form>
  </div>;
}
