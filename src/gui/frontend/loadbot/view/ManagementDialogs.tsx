import { useEffect, useState, type FormEvent } from 'react';
import type { LoadbotActions, LoadbotState, ManagementKind } from '../application/controller';
import { BusyLabel, Button, Checkbox, Dialog, InputControl, StatusDisplay } from '../../ui/components';
import { RecipeBuilder } from './RecipeBuilder';

export type ManagementDialog = Extract<ManagementKind, 'add-catalog' | 'add-project'> | 'recipe-editor';

export function ManagementDialogs({ dialog, state, actions, onClose }: {
  dialog?: ManagementDialog; state: LoadbotState; actions: LoadbotActions; onClose(): void;
}) {
  function close() {
    if (state.management.status === 'submitting') return;
    if (state.pendingProjectAction) {
      actions.cancelProjectAction();
      return;
    }
    if (state.shortcutManagement.pendingDelete) {
      actions.cancelShortcutDeletion();
      return;
    }
    actions.clearManagementStatus();
    actions.closeRecipeEditor();
    onClose();
  }
  useEffect(() => {
    if (!dialog && !state.recipeEditor && !state.shortcutManagement.pendingDelete && !state.pendingProjectAction) return;
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') close(); };
    window.addEventListener('keydown', escape);
    return () => window.removeEventListener('keydown', escape);
  }, [dialog, onClose, state.management.status, state.recipeEditor, state.shortcutManagement.pendingDelete, state.pendingProjectAction]);
  if (state.pendingProjectAction) return <ProjectActionConfirmation state={state} actions={actions} />;
  if (state.shortcutManagement.pendingDelete) return <DeleteShortcutConfirmation state={state} actions={actions} />;
  if (!dialog && !state.recipeEditor) return null;
  if (dialog === 'recipe-editor' || state.recipeEditor) return <RecipeBuilder state={state} actions={actions} onClose={close} onDone={onClose} />;
  if (dialog === 'add-catalog') return <AddCatalogForm state={state} actions={actions} onClose={close} onDone={onClose} />;
  if (dialog === 'add-project') return <AddProjectForm state={state} actions={actions} onClose={close} onDone={onClose} />;
  return null;
}

function ProjectActionConfirmation({ state, actions }: { state: LoadbotState; actions: LoadbotActions }) {
  const pending = state.pendingProjectAction!;
  const busy = state.management.status === 'submitting';
  const reinstall = pending.action === 'reinstall';
  return <Dialog label={reinstall ? 'Reinstall project' : 'Remove project'} onClose={actions.cancelProjectAction}>
    <h2>{reinstall ? 'REINSTALL' : 'REMOVE'} “{pending.project.tool}”?</h2>
    <p>{reinstall
      ? 'This deletes the existing managed checkout and replaces it with a fresh clone from the catalog source.'
      : 'This deletes the local managed checkout. The project remains in the catalog and can be pulled again.'}</p>
    <p><strong>Local changes and local-only commits are never removed.</strong> The backend will refuse this action until they are preserved or discarded explicitly.</p>
    {state.management.status !== 'idle' && (state.management.kind === 'remove-project' || state.management.kind === 'reinstall-project')
      && <StatusDisplay>{state.management.message}</StatusDisplay>}
    <div className="lb-dialog-actions"><Button onClick={actions.cancelProjectAction} disabled={busy}>CANCEL</Button>
      <Button className="lb-danger-action" onClick={() => void actions.confirmProjectAction()} disabled={busy}>
        {busy ? <BusyLabel text={reinstall ? 'REINSTALLING…' : 'REMOVING…'} /> : reinstall ? 'REINSTALL' : 'REMOVE CHECKOUT'}
      </Button></div>
  </Dialog>;
}

function DeleteShortcutConfirmation({ state, actions }: { state: LoadbotState; actions: LoadbotActions }) {
  const shortcuts = state.shortcutManagement.pendingDelete ?? [];
  const busy = state.management.status === 'submitting';
  const count = shortcuts.length;
  return <Dialog label={count === 1 ? 'Delete shortcut' : 'Delete shortcuts'} onClose={actions.cancelShortcutDeletion}>
    <h2>{count === 1 ? `DELETE “${shortcuts[0]?.name}”?` : `DELETE ${count} SHORTCUTS?`}</h2>
    <p>This deletes {count === 1 ? 'the shortcut' : 'these shortcuts'} from Loadbot. It does not delete the tool or any files.</p>
    {count > 1 && <ul className="lb-delete-summary">{shortcuts.slice(0, 8).map((item) => <li key={`${item.catalog}/${item.tool}/${item.name}`}>{item.name}</li>)}
      {count > 8 && <li>…and {count - 8} more</li>}</ul>}
    {state.management.status !== 'idle' && state.management.kind === 'delete-shortcut' && <StatusDisplay>{state.management.message}</StatusDisplay>}
    <div className="lb-dialog-actions"><Button onClick={actions.cancelShortcutDeletion} disabled={busy}>CANCEL</Button>
      <Button className="lb-danger-action" onClick={() => void actions.confirmShortcutDeletion()} disabled={busy}>{busy ? 'DELETING…' : count === 1 ? 'DELETE' : `DELETE ${count}`}</Button></div>
  </Dialog>;
}

function FormStatus({ state, kind }: { state: LoadbotState; kind: ManagementKind }) {
  return state.management.status !== 'idle' && state.management.kind === kind
    ? <StatusDisplay>{state.management.message}</StatusDisplay> : null;
}

function AddCatalogForm({ state, actions, onClose, onDone }: FormProps) {
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [writable, setWritable] = useState(false);
  const busy = state.management.status === 'submitting';
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (await actions.addCatalog({ name: name.trim(), url: url.trim(), writable })) onDone();
  }
  return <Dialog label="Add catalog" onClose={onClose}>
    <h2>ADD CATALOG</h2>
    <form onSubmit={submit}>
      <InputControl autoFocus label="Catalog name" value={name} onChange={(event) => setName(event.target.value)} required disabled={busy} />
      <InputControl label="Git repository URL" value={url} onChange={(event) => setUrl(event.target.value)} required disabled={busy} />
      <Checkbox label="Writable catalog" checked={writable} onChange={(event) => setWritable(event.target.checked)} disabled={busy} />
      <FormStatus state={state} kind="add-catalog" />
      <div className="lb-dialog-actions"><Button onClick={onClose} disabled={busy}>CANCEL</Button><Button type="submit" disabled={busy}>{busy ? 'ADDING…' : 'ADD AND USE'}</Button></div>
    </form>
  </Dialog>;
}

function AddProjectForm({ state, actions, onClose, onDone }: FormProps) {
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [revision, setRevision] = useState('');
  const [commit, setCommit] = useState(false);
  const [push, setPush] = useState(false);
  const busy = state.management.status === 'submitting';
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (await actions.addProject({ name: name.trim(), url: url.trim(), revision: revision.trim() || undefined, commit, push: commit && push })) onDone();
  }
  return <Dialog label="Add project" onClose={onClose}>
    <h2>ADD PROJECT / {state.currentCatalog}</h2>
    <form onSubmit={submit}>
      <InputControl autoFocus label="Project name" value={name} onChange={(event) => setName(event.target.value)} required disabled={busy} />
      <InputControl label="Git repository URL" value={url} onChange={(event) => setUrl(event.target.value)} required disabled={busy} />
      <InputControl label="Revision (optional)" value={revision} onChange={(event) => setRevision(event.target.value)} disabled={busy} />
      <Checkbox label="Commit catalog change" checked={commit} onChange={(event) => { setCommit(event.target.checked); if (!event.target.checked) setPush(false); }} disabled={busy} />
      <Checkbox label="Push catalog commit" checked={push} onChange={(event) => setPush(event.target.checked)} disabled={busy || !commit} />
      <FormStatus state={state} kind="add-project" />
      <div className="lb-dialog-actions"><Button onClick={onClose} disabled={busy}>CANCEL</Button><Button type="submit" disabled={busy}>{busy ? 'ADDING…' : 'ADD PROJECT'}</Button></div>
    </form>
  </Dialog>;
}

interface FormProps { state: LoadbotState; actions: LoadbotActions; onClose(): void; onDone(): void }
