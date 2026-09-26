import { useEffect, useLayoutEffect, useRef } from 'react';
import { FitAddon } from '@xterm/addon-fit';
import { Terminal } from '@xterm/xterm';
import '@xterm/xterm/css/xterm.css';
import type { LoadbotActions, LoadbotState, ProjectTerminalState } from '../application/controller';

function TerminalSurface({ terminalState, actions, visible }: {
  terminalState: ProjectTerminalState;
  actions: LoadbotActions;
  visible: boolean;
}) {
  const host = useRef<HTMLDivElement>(null);
  const terminal = useRef<Terminal | undefined>(undefined);
  const fit = useRef<FitAddon | undefined>(undefined);
  const written = useRef('');
  const sessionKey = useRef('');
  const visibleRef = useRef(visible);
  const lastSize = useRef('');

  visibleRef.current = visible;

  useLayoutEffect(() => {
    if (!host.current) return undefined;
    const emulator = new Terminal({
      allowTransparency: true,
      convertEol: false,
      cursorBlink: true,
      fontFamily: '"DejaVu Sans Mono", "Liberation Mono", monospace',
      fontSize: 12,
      scrollback: 5000,
      theme: {
        background: '#00000000',
        foreground: '#e8eadf',
        cursor: '#e8eadf',
        selectionBackground: '#5f6f6355',
      },
    });
    const fitAddon = new FitAddon();
    emulator.loadAddon(fitAddon);
    emulator.open(host.current);
    terminal.current = emulator;
    fit.current = fitAddon;

    const reportSize = () => {
      const dimensions = `${emulator.cols}x${emulator.rows}`;
      if (dimensions !== lastSize.current && actions.resizeProjectTerminal(emulator.cols, emulator.rows)) {
        lastSize.current = dimensions;
      }
    };
    const fitToHost = () => {
      if (!visibleRef.current || !host.current?.clientWidth || !host.current.clientHeight) return;
      try {
        fitAddon.fit();
        reportSize();
      } catch {
        // A hidden or transitioning pane can be temporarily unmeasurable.
      }
    };
    let fitFrame: number | undefined;
    const scheduleFit = () => {
      if (fitFrame !== undefined) cancelAnimationFrame(fitFrame);
      fitFrame = requestAnimationFrame(() => {
        fitFrame = undefined;
        fitToHost();
      });
    };
    const input = emulator.onData((data) => {
      actions.sendProjectTerminalInput(data);
    });
    const resized = emulator.onResize(reportSize);
    const observer = typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(scheduleFit);
    observer?.observe(host.current);
    scheduleFit();

    return () => {
      if (fitFrame !== undefined) cancelAnimationFrame(fitFrame);
      observer?.disconnect();
      resized.dispose();
      input.dispose();
      emulator.dispose();
      terminal.current = undefined;
      fit.current = undefined;
    };
  }, [actions]);

  useEffect(() => {
    const emulator = terminal.current;
    if (!emulator) return;
    const key = terminalState.launchId
      ?? (terminalState.project ? `project/${terminalState.project.catalog}/${terminalState.project.tool}`
        : `catalog/${terminalState.catalog?.catalog}`);
    if (key !== sessionKey.current) {
      emulator.reset();
      sessionKey.current = key;
      written.current = '';
    }
    if (terminalState.transcript === written.current) return;
    if (terminalState.transcript.startsWith(written.current)) {
      emulator.write(terminalState.transcript.slice(written.current.length));
    } else {
      // The controller bounds retained output. Rebuild only when the oldest
      // retained bytes roll off; normal streaming remains incremental.
      emulator.reset();
      emulator.write(terminalState.transcript);
    }
    written.current = terminalState.transcript;
  }, [terminalState.launchId, terminalState.project, terminalState.transcript]);

  useLayoutEffect(() => {
    if (!visible || !terminal.current || !fit.current) return;
    const frame = requestAnimationFrame(() => {
      if (!host.current?.clientWidth || !host.current.clientHeight) return;
      try {
        fit.current?.fit();
        if (terminalState.status === 'active'
          && actions.resizeProjectTerminal(terminal.current!.cols, terminal.current!.rows)) {
          lastSize.current = `${terminal.current!.cols}x${terminal.current!.rows}`;
        }
        terminal.current?.focus();
      } catch {
        // Layout may still be settling while the drawer opens.
      }
    });
    return () => cancelAnimationFrame(frame);
  }, [actions, terminalState.status, visible]);

  return <div className="lb-terminal-body" role="application" aria-label="Project terminal">
    <div ref={host} className="lb-terminal-emulator" />
  </div>;
}

export function TerminalPane({ state, actions, visible = true }: {
  state: LoadbotState;
  actions: LoadbotActions;
  visible?: boolean;
}) {
  const terminal = state.projectTerminal;
  const selected = state.project;
  const target = terminal.project ?? terminal.catalog;
  const boundToSelection = Boolean(terminal.project && selected
    && terminal.project.catalog === selected.catalog && terminal.project.tool === selected.tool);

  if (!target) return <section hidden={!visible} className="lb-bottom-content lb-project-terminal" role="tabpanel" aria-label="Terminal">
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

  return <section hidden={!visible} className="lb-bottom-content lb-project-terminal" role="tabpanel" aria-label="Terminal">
    <header>
      <div><h2>TERMINAL / {terminal.project?.tool ?? terminal.catalog?.catalog}</h2>
        <span className="lb-metadata">{terminal.project
          ? `${terminal.project.catalog} / ${terminal.project.tool}`
          : `Catalog / ${terminal.catalog?.catalog}`}</span></div>
      <strong data-status={terminal.status}>{status}</strong>
      {(terminal.status === 'active' || terminal.status === 'terminating')
        && <button type="button" disabled={terminal.status === 'terminating'} onClick={() => { void actions.closeProjectTerminal(); }}>Close</button>}
      {(terminal.status === 'exited' || terminal.status === 'error') && <>
        <button type="button" onClick={() => { void actions.restartProjectTerminal(); }}>Restart</button>
        <button type="button" onClick={() => { void actions.closeProjectTerminal(); }}>Close</button>
      </>}
    </header>
    {terminal.project && selected && !boundToSelection && <p className="lb-terminal-binding" role="status">
      This terminal remains bound to {terminal.project.catalog} / {terminal.project.tool}. Close it before starting a terminal for {selected.catalog} / {selected.tool}.
    </p>}
    {terminal.catalog && <p className="lb-terminal-binding" role="status">
      This terminal remains bound to catalog {terminal.catalog.catalog}. Close it before starting another terminal.
    </p>}
    {terminal.message && <p className="lb-command-error" role="alert">{terminal.message}</p>}
    <TerminalSurface terminalState={terminal} actions={actions} visible={visible} />
  </section>;
}
