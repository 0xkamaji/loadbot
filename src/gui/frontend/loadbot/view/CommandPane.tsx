import { useEffect, useRef, useState } from 'react';
import type { CommandResult } from '../application/command';
import type { LoadbotActions, LoadbotState } from '../application/controller';

function errorText(result: Extract<CommandResult, { kind: 'error' }>): string {
  switch (result.code) {
    case 'unknown-command': return `Unknown command: ${result.input}. Type \`help\` for available commands.`;
    case 'unsupported-syntax': return 'Shell syntax is not supported. Enter a Loadbot command; type `help` for available commands.';
    case 'malformed-input': return 'Malformed command: close the quoted name and try again.';
    case 'usage': return `Usage: ${result.usage}`;
    case 'inventory-unavailable': return 'Loadbot inventory is not available. Use Reload Local and try again.';
    case 'project-not-found': return `Project not found: ${result.subject}`;
    case 'project-ambiguous': return `Project name is ambiguous: ${result.subject}`;
    case 'shortcut-not-found': return `Shortcut not found: ${result.subject}`;
    case 'shortcut-ambiguous': return `Shortcut name is ambiguous: ${result.subject}`;
    case 'empty': return '';
  }
}

function Facts({ values }: { values: readonly (readonly [string, string])[] }) {
  return <dl className="lb-command-facts">
    {values.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}
  </dl>;
}

function CommandOutput({ result }: { result: CommandResult }) {
  switch (result.kind) {
    case 'help': return <div className="lb-command-output">
      <p>Available commands:</p>
      <ul>{result.commands.map((command) => <li key={command.name}><code>{command.usage}</code><span>{command.summary}</span></li>)}</ul>
    </div>;
    case 'projects': return <div className="lb-command-output">
      <p>Projects{result.catalog ? ` / ${result.catalog}` : ''}</p>
      {result.projects.length
        ? <ul>{result.projects.map((project) => <li key={`${project.catalog}/${project.tool}`}><code>{project.tool}</code><span>{project.catalog}</span></li>)}</ul>
        : <p className="lb-command-secondary">No projects available in this context.</p>}
    </div>;
    case 'shortcuts': return <div className="lb-command-output">
      <p>Shortcuts / {result.project.catalog}/{result.project.tool}</p>
      {result.shortcuts.length
        ? <ul>{result.shortcuts.map((shortcut) => <li key={`${shortcut.source}:${shortcut.name}:${shortcut.path}`}>
          <code>{shortcut.name}</code><span>{shortcut.source} · {shortcut.path}</span>
        </li>)}</ul>
        : <p className="lb-command-secondary">No shortcuts in this project.</p>}
    </div>;
    case 'project': return <div className="lb-command-output">
      <p>Project</p>
      <Facts values={[
        ['Name', result.project.tool], ['Catalog', result.project.catalog], ['Shortcuts', String(result.project.entries.length)],
      ]} />
    </div>;
    case 'shortcut': return <div className="lb-command-output">
      <p>Shortcut</p>
      <Facts values={[
        ['Name', result.shortcut.name], ['Project', result.project.tool], ['Catalog', result.project.catalog],
        ['Source', result.shortcut.source], ['Path', result.shortcut.path],
        ...(result.shortcut.runner ? [['Runner', result.shortcut.runner] as const] : []),
        ...(result.shortcut.description ? [['Description', result.shortcut.description] as const] : []),
      ]} />
    </div>;
    case 'error': return <div className="lb-command-output lb-command-error">
      <p>{errorText(result)}</p>
      {!!result.choices?.length && <><p className="lb-command-secondary">Use a qualified identity:</p>
        <ul>{result.choices.map((choice) => <li key={choice}><code>{choice}</code></li>)}</ul></>}
    </div>;
  }
}

export function CommandPane({ state, actions }: { state: LoadbotState; actions: LoadbotActions }) {
  const [input, setInput] = useState('');
  const [historyIndex, setHistoryIndex] = useState<number>();
  const draft = useRef('');
  const transcript = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (transcript.current) transcript.current.scrollTop = transcript.current.scrollHeight;
  }, [state.command.entries.length]);

  const moveHistory = (direction: -1 | 1) => {
    const history = state.command.history;
    if (!history.length) return;
    if (direction === -1) {
      if (historyIndex === undefined) draft.current = input;
      const next = historyIndex === undefined ? history.length - 1 : Math.max(0, historyIndex - 1);
      setHistoryIndex(next);
      setInput(history[next]);
      return;
    }
    if (historyIndex === undefined) return;
    const next = historyIndex + 1;
    if (next >= history.length) {
      setHistoryIndex(undefined);
      setInput(draft.current);
    } else {
      setHistoryIndex(next);
      setInput(history[next]);
    }
  };

  return <section className="lb-bottom-content lb-command" role="tabpanel" aria-label="Command">
    <div className="lb-command-transcript" ref={transcript} aria-live="polite">
      <h2>COMMAND / LOADBOT</h2>
      {!state.command.entries.length && <p className="lb-command-secondary">Loadbot command console. Type <code>help</code> for available commands.</p>}
      {state.command.entries.map((entry) => <article key={entry.id}>
        <p className="lb-command-echo"><span aria-hidden="true">&gt;</span> {entry.input}</p>
        <CommandOutput result={entry.result} />
      </article>)}
    </div>
    <form className="lb-command-form" onSubmit={(event) => {
      event.preventDefault();
      if (!actions.submitCommand(input)) return;
      setInput('');
      draft.current = '';
      setHistoryIndex(undefined);
    }}>
      <span aria-hidden="true">&gt;</span>
      <input aria-label="Loadbot command" value={input} autoComplete="off" autoCapitalize="none" spellCheck={false}
        onChange={(event) => {
          setInput(event.target.value);
          draft.current = event.target.value;
          setHistoryIndex(undefined);
        }}
        onKeyDown={(event) => {
          if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
          event.preventDefault();
          moveHistory(event.key === 'ArrowUp' ? -1 : 1);
        }} />
    </form>
  </section>;
}
