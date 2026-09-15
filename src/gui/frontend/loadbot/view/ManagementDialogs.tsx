import { useEffect, useState, type FormEvent } from 'react';
import type { LoadbotRunner } from '../contract';
import type { LoadbotActions, LoadbotState, ManagementKind } from '../application/controller';
import { Button, Checkbox, Dialog, InputControl, SelectControl, StatusDisplay, TextareaControl } from '../../ui/components';

export type ManagementDialog = Extract<ManagementKind, 'add-catalog' | 'add-project' | 'add-shortcut'>;

export function ManagementDialogs({ dialog, state, actions, onClose }: {
  dialog?: ManagementDialog; state: LoadbotState; actions: LoadbotActions; onClose(): void;
}) {
  useEffect(() => {
    if (!dialog) return;
    const close = (event: KeyboardEvent) => { if (event.key === 'Escape' && state.management.status !== 'submitting') onClose(); };
    window.addEventListener('keydown', close);
    return () => window.removeEventListener('keydown', close);
  }, [dialog, onClose, state.management.status]);
  if (!dialog) return null;
  const close = () => {
    if (state.management.status === 'submitting') return;
    actions.clearManagementStatus();
    onClose();
  };
  if (dialog === 'add-catalog') return <AddCatalogForm state={state} actions={actions} onClose={close} onDone={onClose} />;
  if (dialog === 'add-project') return <AddProjectForm state={state} actions={actions} onClose={close} onDone={onClose} />;
  return <AddShortcutForm state={state} actions={actions} onClose={close} onDone={onClose} />;
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

function AddShortcutForm({ state, actions, onClose, onDone }: FormProps) {
  const [name, setName] = useState('');
  const [path, setPath] = useState('');
  const [description, setDescription] = useState('');
  const [runner, setRunner] = useState<LoadbotRunner | ''>('');
  const busy = state.management.status === 'submitting';
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (await actions.addShortcut({
      name: name.trim(), path: path.trim(), description: description.trim() || undefined,
      runner: runner || undefined,
    })) onDone();
  }
  return <Dialog label="Add shortcut" onClose={onClose}>
    <h2>ADD SHORTCUT / {state.project?.tool}</h2>
    <form onSubmit={submit}>
      <InputControl autoFocus label="Shortcut name" value={name} onChange={(event) => setName(event.target.value)} required disabled={busy} />
      <InputControl label="Repository-relative path" value={path} onChange={(event) => setPath(event.target.value)} required disabled={busy} placeholder="scripts/example.py" />
      <TextareaControl label="Description (optional)" value={description} onChange={(event) => setDescription(event.target.value)} disabled={busy} />
      <SelectControl label="Runner (optional)" value={runner} onChange={(event) => setRunner(event.target.value as LoadbotRunner | '')} disabled={busy}>
        <option value="">Use file association</option><option value="direct">Direct</option><option value="bash">Bash</option>
        <option value="sh">sh</option><option value="python">Python</option><option value="powershell">PowerShell</option>
      </SelectControl>
      <FormStatus state={state} kind="add-shortcut" />
      <div className="lb-dialog-actions"><Button onClick={onClose} disabled={busy}>CANCEL</Button><Button type="submit" disabled={busy}>{busy ? 'ADDING…' : 'ADD SHORTCUT'}</Button></div>
    </form>
  </Dialog>;
}

interface FormProps { state: LoadbotState; actions: LoadbotActions; onClose(): void; onDone(): void }
