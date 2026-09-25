import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from 'react';
import type { LoadbotActions, LoadbotState } from '../application/controller';

export function TerminalPane({ state, actions }: { state: LoadbotState; actions: LoadbotActions }) {
  const terminal = state.projectTerminal;
  const selected = state.project;
  const output = useRef<HTMLPreElement>(null);
  const [input, setInput] = useState('');
  const boundToSelection = Boolean(terminal.project && selected
    && terminal.project.catalog === selected.catalog && terminal.project.tool === selected.tool);

  useEffect(() => {
    if (output.current) output.current.scrollTop = output.current.scrollHeight;
  }, [terminal.transcript]);

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!input || !actions.sendProjectTerminalInput(`${input}\r`)) return;
    setInput('');
  }

  function control(event: KeyboardEvent<HTMLInputElement>) {
    if (event.ctrlKey && !event.altKey && !event.metaKey && event.key.toLowerCase() === 'c') {
      event.preventDefault();
      if (actions.sendProjectTerminalInput('\u0003')) setInput('');
    }
  }

  if (!terminal.project) return <section className="lb-bottom-content lb-project-terminal" role="tabpanel" aria-label="Terminal">
    <h2>TERMINAL / PROJECT</h2>
    {!selected && <p className="lb-metadata">Select a project to open its terminal.</p>}
    {selected?.installed === false && <p className="lb-metadata">Install this project before opening a terminal.</p>}
    {selected && selected.installed !== false && <p className="lb-metadata">Open Terminal to start a shell for the selected project.</p>}
  </section>;

  const status = terminal.status === 'active' ? 'ACTIVE'
    : terminal.status === 'starting' ? 'STARTING'
      : terminal.status === 'terminating' ? 'CLOSING'
        : terminal.status === 'exited' ? `EXITED ${terminal.exitCode ?? ''}`.trim()
          : terminal.status === 'error' ? 'ERROR' : 'IDLE';

  return <section className="lb-bottom-content lb-project-terminal" role="tabpanel" aria-label="Terminal">
    <header>
      <div><h2>TERMINAL / {terminal.project.tool}</h2>
        <span className="lb-metadata">{terminal.project.catalog} / {terminal.project.tool}</span></div>
      <strong data-status={terminal.status}>{status}</strong>
      {(terminal.status === 'active' || terminal.status === 'terminating')
        && <button type="button" disabled={terminal.status === 'terminating'} onClick={() => { void actions.closeProjectTerminal(); }}>Close</button>}
      {(terminal.status === 'exited' || terminal.status === 'error') && <>
        <button type="button" onClick={() => { void actions.restartProjectTerminal(); }}>Restart</button>
        <button type="button" onClick={() => { void actions.closeProjectTerminal(); }}>Close</button>
      </>}
    </header>
    {selected && !boundToSelection && <p className="lb-terminal-binding" role="status">
      This terminal remains bound to {terminal.project.catalog} / {terminal.project.tool}. Close it before starting a terminal for {selected.catalog} / {selected.tool}.
    </p>}
    {terminal.message && <p className="lb-command-error" role="alert">{terminal.message}</p>}
    <pre ref={output} className="lb-terminal-transcript" aria-label="Project terminal output" aria-live="polite">{terminal.transcript}</pre>
    <form className="lb-command-form" onSubmit={submit}>
      <span aria-hidden="true">$</span>
      <input aria-label="Project terminal input" value={input} onChange={(event) => setInput(event.currentTarget.value)}
        onKeyDown={control} disabled={terminal.status !== 'active'} autoComplete="off" autoCapitalize="none" spellCheck={false} />
    </form>
  </section>;
}
