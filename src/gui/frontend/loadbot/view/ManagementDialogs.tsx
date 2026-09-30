import { useEffect, useState, type FormEvent } from 'react';
import type { LoadbotActions, LoadbotState, ManagementKind } from '../application/controller';
import { BusyLabel, Button, Checkbox, Dialog, InputControl, StatusDisplay } from '../../ui/components';

export type ManagementDialog = Extract<ManagementKind, 'add-catalog' | 'create-catalog' | 'connect-catalog' | 'add-project'> | 'manage-catalog';

export function ManagementDialogs({ dialog, state, actions, onClose, onOpen }: {
  dialog?: ManagementDialog; state: LoadbotState; actions: LoadbotActions; onClose(): void; onOpen(dialog: ManagementDialog): void;
}) {
  function close() {
    if (state.management.status === 'submitting') return;
    if (state.pendingProjectAction) {
      actions.cancelProjectAction();
      return;
    }
    if (state.pendingCatalogAction) {
      actions.cancelCatalogAction();
      return;
    }
    if (state.pendingCommitPush) {
      actions.cancelCommitPush();
      return;
    }
    actions.clearManagementStatus();
    onClose();
  }
  useEffect(() => {
    if (!dialog && !state.pendingProjectAction && !state.pendingCatalogAction && !state.pendingCommitPush) return;
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') close(); };
    window.addEventListener('keydown', escape);
    return () => window.removeEventListener('keydown', escape);
  }, [dialog, onClose, state.management.status, state.pendingProjectAction, state.pendingCatalogAction, state.pendingCommitPush]);
  if (state.pendingCommitPush) return <CommitPushDialog state={state} actions={actions} />;
  if (state.pendingProjectAction) return <ProjectActionConfirmation state={state} actions={actions} />;
  if (state.pendingCatalogAction) return <CatalogActionConfirmation state={state} actions={actions} onDone={onClose} />;
  if (state.command.interactive && state.management.status === 'submitting') return null;
  if (!dialog) return null;
  if (dialog === 'add-catalog') return <AddCatalogForm state={state} actions={actions} onClose={close} onDone={onClose} />;
  if (dialog === 'create-catalog') return <CreateCatalogForm state={state} actions={actions} onClose={close} onDone={onClose} />;
  if (dialog === 'connect-catalog') return <ConnectCatalogForm state={state} actions={actions} onClose={close} onDone={onClose} />;
  if (dialog === 'add-project') return <AddProjectForm state={state} actions={actions} onClose={close} onDone={onClose} />;
  if (dialog === 'manage-catalog') return <ManageCatalog state={state} actions={actions} onClose={close} onConnect={() => onOpen('connect-catalog')} />;
  return null;
}

function CommitPushDialog({ state, actions }: { state: LoadbotState; actions: LoadbotActions }) {
  const pending = state.pendingCommitPush!;
  const busy = state.management.status === 'submitting';
  const valid = pending.selectedPaths.length > 0 && pending.commitMessage.trim().length > 0;
  function submit(event: FormEvent) {
    event.preventDefault();
    if (valid) void actions.confirmCommitPush();
  }
  return <Dialog label="Commit & Push" onClose={actions.cancelCommitPush}>
    <h2>COMMIT &amp; PUSH</h2>
    <p>Project: <strong>{pending.project.tool}</strong></p>
    <form onSubmit={submit}>
      <fieldset className="lb-commit-files" disabled={busy}>
        <legend>Changed files</legend>
        {pending.changedFiles.map((change) => <label key={change.path}>
          <input type="checkbox" checked={pending.selectedPaths.includes(change.path)}
            onChange={() => actions.toggleCommitPushPath(change.path)} />
          <span><strong>{change.status}</strong> {change.originalPath ? `${change.originalPath} → ` : ''}{change.path}</span>
        </label>)}
      </fieldset>
      {!pending.selectedPaths.length && <StatusDisplay>Select at least one changed file.</StatusDisplay>}
      <InputControl autoFocus label="Commit message" value={pending.commitMessage}
        onChange={(event) => actions.setCommitPushMessage(event.target.value)} required disabled={busy} />
      <div className="lb-dialog-actions">
        <Button onClick={actions.cancelCommitPush} disabled={busy}>CANCEL</Button>
        <Button type="submit" disabled={busy || !valid}>{busy ? <BusyLabel text="COMMITTING…" /> : 'Commit & Push'}</Button>
      </div>
    </form>
  </Dialog>;
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

function CatalogActionConfirmation({ state, actions, onDone }: {
  state: LoadbotState; actions: LoadbotActions; onDone(): void;
}) {
  const pending = state.pendingCatalogAction!;
  const busy = state.management.status === 'submitting';
  const deletingLocal = pending.action === 'delete-local';
  const deletingTools = pending.action === 'delete-with-managed-tools';
  const title = pending.action === 'unregister' ? 'REMOVE REGISTRATION'
    : deletingLocal ? 'DELETE LOCAL CATALOG' : 'DELETE CATALOG & MANAGED TOOLS';
  const confirmLabel = pending.action === 'unregister' ? 'REMOVE REGISTRATION'
    : deletingLocal ? 'DELETE LOCAL CATALOG' : 'DELETE CATALOG & TOOLS';
  const ready = pending.action === 'unregister' || Boolean(pending.plan);
  async function confirm() {
    if (await actions.confirmCatalogAction()) onDone();
  }
  return <Dialog label={`${title.toLowerCase()} ${pending.catalog}`} onClose={actions.cancelCatalogAction}>
    <h2>{title} / {pending.catalog}?</h2>
    {pending.action === 'unregister' && <>
      <p>This removes the catalog registration from Loadbot. The local catalog directory and every managed tool checkout remain on disk.</p>
      <p>The preserved files remain available for manual recovery.</p>
    </>}
    {deletingLocal && <>
      <p>This removes the catalog registration and permanently deletes its local catalog directory.</p>
      <p><strong>Managed tool checkouts remain on disk.</strong></p>
    </>}
    {deletingTools && <>
      <p>This removes the catalog registration and permanently deletes its local catalog directory and every authoritative managed tool checkout listed below.</p>
      <p><strong>Tools from other catalogs, foreign repositories, modified checkouts, and local-only commits are not deleted.</strong></p>
    </>}
    {deletingTools && pending.plan && <div className="lb-catalog-delete-preview">
      <h3>{`MANAGED TOOL CHECKOUTS TO DELETE (${pending.plan.managedTools.length})`}</h3>
      {pending.plan.managedTools.length
        ? <ul className="lb-delete-summary">{pending.plan.managedTools.map((item) => <li key={item.tool}>{item.tool}</li>)}</ul>
        : <p>No managed tool checkouts were found for this catalog.</p>}
    </div>}
    {state.management.status !== 'idle' && (state.management.kind === 'unregister-catalog'
      || state.management.kind === 'delete-local-catalog' || state.management.kind === 'delete-catalog-with-managed-tools')
      && <StatusDisplay>{state.management.message}</StatusDisplay>}
    <div className="lb-dialog-actions"><Button onClick={actions.cancelCatalogAction} disabled={busy}>CANCEL</Button>
      <Button className="lb-danger-action" onClick={() => { void confirm(); }} disabled={busy || !ready}>
        {busy ? <BusyLabel text={pending.plan || pending.action === 'unregister' ? 'WORKING…' : 'INSPECTING…'} /> : confirmLabel}
      </Button></div>
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
  const valid = name.trim().length > 0 && url.trim().length > 0;
  return <Dialog label="Add existing catalog" onClose={onClose}>
    <h2>ADD EXISTING CATALOG</h2>
    <p>Connect an existing valid Loadbot catalog repository.</p>
    <form onSubmit={submit}>
      <InputControl autoFocus label="Catalog name" value={name} onChange={(event) => setName(event.target.value)} required disabled={busy} />
      <InputControl label="Git repository URL" value={url} onChange={(event) => setUrl(event.target.value)} required disabled={busy} />
      <Checkbox label="Writable catalog" checked={writable} onChange={(event) => setWritable(event.target.checked)} disabled={busy} />
      <FormStatus state={state} kind="add-catalog" />
      <div className="lb-dialog-actions"><Button onClick={onClose} disabled={busy}>CANCEL</Button><Button type="submit" disabled={busy || !valid}>{busy ? 'ADDING…' : 'ADD AND USE'}</Button></div>
    </form>
  </Dialog>;
}

function CreateCatalogForm({ state, actions, onClose, onDone }: FormProps) {
  const [name, setName] = useState('');
  const [backend, setBackend] = useState<'local' | 'git'>('local');
  const [url, setUrl] = useState('');
  const [commit, setCommit] = useState(false);
  const [push, setPush] = useState(false);
  const busy = state.management.status === 'submitting';
  const valid = name.trim().length > 0 && (backend === 'local' || url.trim().length > 0);
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!valid) return;
    const input = backend === 'local'
      ? { name: name.trim(), backend } as const
      : { name: name.trim(), backend, url: url.trim(), commit, push: commit && push } as const;
    if (await actions.createCatalog(input)) onDone();
  }
  return <Dialog label="Create new catalog" onClose={onClose}>
    <h2>CREATE NEW CATALOG</h2>
    <p>Create a writable catalog in Loadbot's managed catalog area.</p>
    <form onSubmit={submit}>
      <InputControl autoFocus label="Catalog name" value={name} onChange={(event) => setName(event.target.value)} required disabled={busy} />
      <fieldset className="lb-storage-options" disabled={busy}>
        <legend>Storage</legend>
        <label><input type="radio" name="catalog-storage" checked={backend === 'local'}
          onChange={() => { setBackend('local'); setCommit(false); setPush(false); }} /> Local only</label>
        <label><input type="radio" name="catalog-storage" checked={backend === 'git'} onChange={() => setBackend('git')} /> Git-backed</label>
      </fieldset>
      {backend === 'git' && <>
        <p>Initialize an existing empty Git repository. Loadbot will not create a remote repository.</p>
        <InputControl label="Empty Git repository URL" value={url} onChange={(event) => setUrl(event.target.value)} required disabled={busy} />
        <Checkbox label="Commit initial catalog.toml" checked={commit}
          onChange={(event) => { setCommit(event.target.checked); if (!event.target.checked) setPush(false); }} disabled={busy} />
        <Checkbox label="Push initial catalog commit" checked={push} onChange={(event) => setPush(event.target.checked)} disabled={busy || !commit} />
      </>}
      <FormStatus state={state} kind="create-catalog" />
      <div className="lb-dialog-actions"><Button onClick={onClose} disabled={busy}>CANCEL</Button>
        <Button type="submit" disabled={busy || !valid}>{busy ? <BusyLabel text="CREATING…" /> : 'CREATE AND USE'}</Button></div>
    </form>
  </Dialog>;
}

function ConnectCatalogForm({ state, actions, onClose, onDone }: FormProps) {
  const [catalog] = useState(state.currentCatalog ?? '');
  const [url, setUrl] = useState('');
  const [commit, setCommit] = useState(false);
  const [push, setPush] = useState(false);
  const busy = state.management.status === 'submitting';
  const valid = url.trim().length > 0;
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!valid) return;
    if (await actions.connectCatalog({ name: catalog, url: url.trim(), commit, push: commit && push })) onDone();
  }
  return <Dialog label={`Connect catalog ${catalog} to Git`} onClose={onClose}>
    <h2>CONNECT TO REPO / {catalog}</h2>
    <p>Promote this local catalog in place. The remote must already exist and contain no refs; Loadbot will never merge or overwrite it.</p>
    <form onSubmit={submit}>
      <InputControl autoFocus label="Empty Git repository URL" value={url} onChange={(event) => setUrl(event.target.value)} required disabled={busy} />
      <Checkbox label="Commit catalog.toml" checked={commit}
        onChange={(event) => { setCommit(event.target.checked); if (!event.target.checked) setPush(false); }} disabled={busy} />
      <Checkbox label="Push catalog commit" checked={push} onChange={(event) => setPush(event.target.checked)} disabled={busy || !commit} />
      <FormStatus state={state} kind="connect-catalog" />
      <div className="lb-dialog-actions"><Button onClick={onClose} disabled={busy}>CANCEL</Button>
        <Button type="submit" disabled={busy || !valid}>{busy ? <BusyLabel text="CONNECTING…" /> : 'CONNECT TO REPO'}</Button></div>
    </form>
  </Dialog>;
}

function ManageCatalog({ state, actions, onClose, onConnect }: Omit<FormProps, 'onDone'> & { onConnect(): void }) {
  const catalog = state.catalogState.status === 'ready'
    ? state.catalogState.catalogs.find((item) => item.name === state.currentCatalog) : undefined;
  if (!catalog) return null;
  const busy = state.management.status === 'submitting' || state.catalogFolder.status === 'opening';
  const terminalInCatalog = ['starting', 'active', 'terminating'].includes(state.projectTerminal.status)
    && (state.projectTerminal.project?.catalog === catalog.name || state.projectTerminal.catalog?.catalog === catalog.name);
  return <Dialog label={`Manage catalog ${catalog.name}`} onClose={onClose}>
    <h2>MANAGE CATALOG / {catalog.name}</h2>
    <dl className="lb-catalog-details">
      <dt>Type</dt><dd>{catalog.backend === 'git' ? 'Git-backed' : 'Local only'}</dd>
      <dt>Writable</dt><dd>{catalog.writable ? 'yes' : 'no'}</dd>
      <dt>State</dt><dd>{catalog.state}</dd>
      {catalog.backend === 'git' && <><dt>Remote</dt><dd>{catalog.url}</dd></>}
    </dl>
    {state.catalogFolder.status !== 'idle' && state.catalogFolder.catalog === catalog.name
      && <StatusDisplay>{state.catalogFolder.status === 'opening' ? 'Opening catalog folder…' : state.catalogFolder.message}</StatusDisplay>}
    <div className="lb-dialog-actions lb-catalog-manage-actions">
      <Button onClick={onClose} disabled={busy}>CLOSE</Button>
      <Button onClick={actions.openCatalogFolder} disabled={busy || catalog.state !== 'installed'}>OPEN FOLDER</Button>
      <Button onClick={() => { actions.openCatalogTerminal(); onClose(); }} disabled={busy || catalog.state !== 'installed'}>OPEN TERMINAL</Button>
      {catalog.backend === 'local' && catalog.writable && <Button disabled={busy || catalog.state !== 'installed'} onClick={onConnect}>CONNECT TO REPO</Button>}
      {catalog.backend === 'git' && <Button disabled={busy || catalog.state !== 'installed'}
        onClick={() => { void actions.syncCatalog(); }}>REFRESH CATALOG</Button>}
    </div>
    <div className="lb-catalog-danger-zone">
      <h3>REMOVE OR DELETE</h3>
      <p>Choose exactly what Loadbot should remove. Each action requires confirmation.</p>
      {terminalInCatalog && <StatusDisplay>Close the active terminal for this catalog before removing it.</StatusDisplay>}
      <Button disabled={busy || terminalInCatalog} onClick={() => actions.requestCatalogAction('unregister')}>REMOVE REGISTRATION ONLY</Button>
      <Button className="lb-danger-action" disabled={busy || terminalInCatalog || catalog.state !== 'installed'}
        onClick={() => actions.requestCatalogAction('delete-local')}>DELETE LOCAL CATALOG, KEEP TOOLS</Button>
      <Button className="lb-danger-action" disabled={busy || terminalInCatalog || catalog.state !== 'installed'}
        onClick={() => actions.requestCatalogAction('delete-with-managed-tools')}>DELETE CATALOG &amp; MANAGED TOOLS</Button>
    </div>
  </Dialog>;
}

function AddProjectForm({ state, actions, onClose, onDone }: FormProps) {
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [revision, setRevision] = useState('');
  const [commit, setCommit] = useState(false);
  const [push, setPush] = useState(false);
  const busy = state.management.status === 'submitting';
  const catalog = state.catalogState.status === 'ready'
    ? state.catalogState.catalogs.find((item) => item.name === state.currentCatalog) : undefined;
  const gitBacked = catalog?.backend === 'git';
  const valid = name.trim().length > 0 && url.trim().length > 0;
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!valid) return;
    if (await actions.addProject({
      name: name.trim(), url: url.trim(), revision: revision.trim() || undefined,
      commit: gitBacked && commit, push: gitBacked && commit && push,
    })) onDone();
  }
  return <Dialog label="Add project" onClose={onClose}>
    <h2>ADD PROJECT / {state.currentCatalog}</h2>
    <form onSubmit={submit}>
      <InputControl autoFocus label="Project name" value={name} onChange={(event) => setName(event.target.value)} required disabled={busy} />
      <InputControl label="Git repository URL" value={url} onChange={(event) => setUrl(event.target.value)} required disabled={busy} />
      <InputControl label="Revision (optional)" value={revision} onChange={(event) => setRevision(event.target.value)} disabled={busy} />
      {gitBacked && <>
        <Checkbox label="Commit catalog change" checked={commit} onChange={(event) => { setCommit(event.target.checked); if (!event.target.checked) setPush(false); }} disabled={busy} />
        <Checkbox label="Push catalog commit" checked={push} onChange={(event) => setPush(event.target.checked)} disabled={busy || !commit} />
      </>}
      <FormStatus state={state} kind="add-project" />
      <div className="lb-dialog-actions"><Button onClick={onClose} disabled={busy}>CANCEL</Button><Button type="submit" disabled={busy || !valid}>{busy ? 'ADDING…' : 'ADD PROJECT'}</Button></div>
    </form>
  </Dialog>;
}

interface FormProps { state: LoadbotState; actions: LoadbotActions; onClose(): void; onDone(): void }
