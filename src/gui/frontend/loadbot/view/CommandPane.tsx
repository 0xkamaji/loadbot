import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { applyCommandCompletion, type CommandCompletion, type CommandCompletionCandidate, type CommandResult } from '../application/command';
import type { LoadbotActions, LoadbotState } from '../application/controller';

interface CompletionSession {
  readonly completion: CommandCompletion;
  readonly selectedId: string;
}

export interface CompletionCandidateRect {
  readonly left: number;
  readonly right: number;
  readonly top: number;
  readonly bottom: number;
}

/** Find the nearest horizontal candidate on the adjacent rendered row. */
export function visualCompletionNeighbor(
  rects: readonly CompletionCandidateRect[],
  currentIndex: number,
  direction: -1 | 1,
): number {
  if (rects.length < 2 || !rects[currentIndex]) return currentIndex;
  const rows: number[][] = [];
  rects.forEach((rect, index) => {
    const row = rows.at(-1);
    const rowTop = row?.length ? rects[row[0]].top : undefined;
    if (row && rowTop !== undefined && Math.abs(rowTop - rect.top) <= 2) row.push(index);
    else rows.push([index]);
  });
  const rowIndex = rows.findIndex((row) => row.includes(currentIndex));
  if (rowIndex < 0) return currentIndex;
  const targetRow = rows[(rowIndex + direction + rows.length) % rows.length];
  const center = (rects[currentIndex].left + rects[currentIndex].right) / 2;
  return targetRow.reduce((nearest, candidate) => {
    const candidateCenter = (rects[candidate].left + rects[candidate].right) / 2;
    const nearestCenter = (rects[nearest].left + rects[nearest].right) / 2;
    return Math.abs(candidateCenter - center) < Math.abs(nearestCenter - center) ? candidate : nearest;
  }, targetRow[0]);
}

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
  const [completionSession, setCompletionSession] = useState<CompletionSession>();
  const draft = useRef('');
  const transcript = useRef<HTMLDivElement>(null);
  const inputElement = useRef<HTMLInputElement>(null);
  const candidateElements = useRef(new Map<string, HTMLElement>());
  const pendingCaret = useRef<number | undefined>(undefined);
  const completionId = useId();

  useEffect(() => {
    if (transcript.current) transcript.current.scrollTop = transcript.current.scrollHeight;
  }, [state.command.entries.length]);

  useLayoutEffect(() => {
    if (pendingCaret.current === undefined) return;
    inputElement.current?.focus();
    inputElement.current?.setSelectionRange(pendingCaret.current, pendingCaret.current);
    pendingCaret.current = undefined;
  }, [input]);

  useEffect(() => {
    if (!completionSession) return;
    const selected = candidateElements.current.get(completionSession.selectedId);
    selected?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
  }, [completionSession]);

  const setInputAtCaret = (nextInput: string, caret: number) => {
    pendingCaret.current = caret;
    setInput(nextInput);
    draft.current = nextInput;
    setHistoryIndex(undefined);
  };

  const acceptCompletion = (candidate?: CommandCompletionCandidate) => {
    if (!completionSession) return;
    const selected = candidate ?? completionSession.completion.candidates.find((item) => item.id === completionSession.selectedId);
    if (!selected) return;
    const applied = applyCommandCompletion(input, completionSession.completion, selected);
    setCompletionSession(undefined);
    setInputAtCaret(applied.input, applied.caret);
  };

  const beginOrRefreshCompletion = (nextInput: string, caret: number, completeUnique: boolean) => {
    const completion = actions.completeCommand(nextInput, caret);
    if (!completion) {
      setCompletionSession(undefined);
      return;
    }
    if (completion.candidates.length === 1 && completeUnique) {
      const applied = applyCommandCompletion(nextInput, completion, completion.candidates[0]);
      setCompletionSession(undefined);
      setInputAtCaret(applied.input, applied.caret);
      return;
    }
    const selectedId = completionSession && completion.candidates.some((candidate) => candidate.id === completionSession.selectedId)
      ? completionSession.selectedId
      : completion.candidates[0].id;
    setCompletionSession({ completion, selectedId });
  };

  const selectCandidate = (direction: -1 | 1) => {
    if (!completionSession) return;
    const candidates = completionSession.completion.candidates;
    const current = Math.max(0, candidates.findIndex((candidate) => candidate.id === completionSession.selectedId));
    const next = (current + direction + candidates.length) % candidates.length;
    setCompletionSession({ ...completionSession, selectedId: candidates[next].id });
  };

  const selectVisualRow = (direction: -1 | 1) => {
    if (!completionSession) return;
    const candidates = completionSession.completion.candidates;
    const current = Math.max(0, candidates.findIndex((candidate) => candidate.id === completionSession.selectedId));
    const rects = candidates.map((candidate) => candidateElements.current.get(candidate.id)?.getBoundingClientRect())
      .filter((rect): rect is DOMRect => rect !== undefined);
    if (rects.length !== candidates.length) return;
    const next = visualCompletionNeighbor(rects, current, direction);
    setCompletionSession({ ...completionSession, selectedId: candidates[next].id });
  };

  const moveHistory = (direction: -1 | 1) => {
    const history = state.command.history;
    if (!history.length) return;
    if (direction === -1) {
      if (historyIndex === undefined) draft.current = input;
      const next = historyIndex === undefined ? history.length - 1 : Math.max(0, historyIndex - 1);
      setHistoryIndex(next);
      setInput(history[next]);
      setCompletionSession(undefined);
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
    setCompletionSession(undefined);
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
      if (completionSession) {
        acceptCompletion();
        return;
      }
      if (!actions.submitCommand(input)) return;
      setInput('');
      draft.current = '';
      setHistoryIndex(undefined);
      setCompletionSession(undefined);
    }}>
      <span aria-hidden="true">&gt;</span>
      <input ref={inputElement} aria-label="Loadbot command" value={input} autoComplete="off" autoCapitalize="none" spellCheck={false}
        role="combobox" aria-autocomplete="list" aria-expanded={completionSession !== undefined}
        aria-controls={completionSession ? completionId : undefined}
        aria-activedescendant={completionSession
          ? `${completionId}-option-${completionSession.completion.candidates.findIndex((candidate) => candidate.id === completionSession.selectedId)}`
          : undefined}
        onChange={(event) => {
          const value = event.currentTarget.value;
          const caret = event.currentTarget.selectionStart ?? value.length;
          setInput(value);
          draft.current = value;
          setHistoryIndex(undefined);
          if (completionSession) beginOrRefreshCompletion(value, caret, true);
        }}
        onKeyDown={(event) => {
          if (completionSession) {
            if (event.key === 'Tab') {
              event.preventDefault();
              selectCandidate(event.shiftKey ? -1 : 1);
            } else if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
              event.preventDefault();
              selectCandidate(event.key === 'ArrowRight' ? 1 : -1);
            } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
              event.preventDefault();
              selectVisualRow(event.key === 'ArrowDown' ? 1 : -1);
            } else if (event.key === 'Enter') {
              event.preventDefault();
              acceptCompletion();
            } else if (event.key === 'Escape') {
              event.preventDefault();
              setCompletionSession(undefined);
            }
            return;
          }
          if (event.key === 'Tab') {
            event.preventDefault();
            beginOrRefreshCompletion(input, event.currentTarget.selectionStart ?? input.length, true);
            return;
          }
          if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
          event.preventDefault();
          moveHistory(event.key === 'ArrowUp' ? -1 : 1);
        }} />
    </form>
    {completionSession && <div id={completionId} className="lb-command-completions" role="listbox" aria-label="Command completions">
      {completionSession.completion.candidates.map((candidate, index) => <button
        id={`${completionId}-option-${index}`} key={candidate.id} type="button" role="option" tabIndex={-1}
        aria-selected={candidate.id === completionSession.selectedId}
        ref={(element) => {
          if (element) candidateElements.current.set(candidate.id, element);
          else candidateElements.current.delete(candidate.id);
        }}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => acceptCompletion(candidate)}
      >{candidate.label}</button>)}
    </div>}
  </section>;
}
