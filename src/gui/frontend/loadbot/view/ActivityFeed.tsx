import { useEffect, useState } from 'react';
import type { ActivityEntry, ActivityLogEntry, LoadbotActions, LoadbotState } from '../application/controller';
import { CommandPane } from './CommandPane';
import { TerminalPane } from './TerminalPane';

const operationLabels: Record<ActivityEntry['operation'], string> = {
  'catalog-sync': 'Refresh Catalog',
  'catalog-add': 'Add Catalog',
  'catalog-create': 'Create Catalog',
  'project-add': 'Add Project',
  'shortcut-add': 'Add Shortcut',
  'shortcut-update': 'Update Shortcut',
  'shortcut-delete': 'Delete Shortcut',
  'local-reload': 'Reload',
  'project-folder-open': 'Open Project Folder',
  'project-pull': 'Pull Project',
  'project-push': 'Push Project',
  'project-update': 'Update Project',
  'project-remove': 'Remove Project',
  'project-reinstall': 'Reinstall Project',
};

type GroupStatus = 'running' | 'completed' | 'failed' | 'cancelled';
interface ActivityGroup {
  readonly id: string;
  readonly entries: readonly ActivityEntry[];
  readonly logs: readonly ActivityLogEntry[];
  readonly status: GroupStatus;
}

function context(entry: ActivityEntry): string {
  return [entry.catalog, entry.project, entry.shortcut].filter(Boolean).join(' / ');
}

function description(entry: ActivityEntry): string {
  const target = context(entry);
  switch (entry.stage) {
    case 'started': return `${operationLabels[entry.operation]} started${target ? `: ${target}` : ''}`;
    case 'validating': return `Validating configured catalog: ${entry.catalog}`;
    case 'repository-checked': return `Configured catalog repository verified: ${entry.catalog}`;
    case 'updating-repository': return `Updating catalog from its configured Git remote: ${entry.catalog}`;
    case 'current': return `Catalog is already current: ${entry.catalog}${entry.detail ? ` (${entry.detail})` : ''}`;
    case 'updated': return `Catalog updated: ${entry.catalog}${entry.detail ? ` (${entry.detail})` : ''}`;
    case 'validating-checkout': return `Validating existing checkout: ${target}`;
    case 'inspecting-repository': return `Inspecting repository changes: ${target}`;
    case 'awaiting-commit': return `Awaiting Commit & Push confirmation: ${target}`;
    case 'staging-changes': return `Staging selected changes: ${target}`;
    case 'creating-commit': return `Creating commit: ${target}`;
    case 'pushing-commits': return `Pushing commits: ${target}`;
    case 'interactive-authentication': return 'Interactive authentication required';
    case 'cloning-project': return `${entry.operation === 'project-reinstall' ? 'Cloning fresh checkout' : 'Cloning project'}: ${target}`;
    case 'validating-fresh-checkout': return `Validating fresh checkout: ${target}`;
    case 'fetching-and-updating': return `Fetching remote and updating checkout: ${target}`;
    case 'removing-checkout': return `Removing local checkout: ${target}`;
    case 'replacing-checkout': return `Replacing checkout: ${target}`;
    case 'authoritative-reload': return 'Rereading authoritative local inventory and catalog state';
    case 'catalog-state': return `Catalog state: ${entry.catalog} · ${entry.detail}`;
    case 'completed': return entry.detail ?? `${operationLabels[entry.operation]} completed`;
    case 'failed': return `${operationLabels[entry.operation]} failed${entry.detail ? `: ${entry.detail}` : ''}`;
    case 'cancelled': return `${operationLabels[entry.operation]} cancelled${entry.detail ? `: ${entry.detail}` : ''}`;
    default: return entry.detail ?? entry.stage;
  }
}

function groupStatus(entries: readonly ActivityEntry[]): GroupStatus {
  const terminal = [...entries].reverse().find((entry) => ['completed', 'failed', 'cancelled'].includes(entry.stage));
  return terminal?.stage === 'completed' ? 'completed' : terminal?.stage === 'failed' ? 'failed'
    : terminal?.stage === 'cancelled' ? 'cancelled' : 'running';
}

function activityGroups(state: LoadbotState): readonly ActivityGroup[] {
  const entries = new Map<string, ActivityEntry[]>();
  for (const entry of state.activity) {
    const group = entries.get(entry.operationId) ?? [];
    group.push(entry);
    entries.set(entry.operationId, group);
  }
  const logs = new Map<string, ActivityLogEntry[]>();
  for (const log of state.activityLogs) {
    const group = logs.get(log.operationId) ?? [];
    group.push(log);
    logs.set(log.operationId, group);
  }
  return [...entries].map(([id, group]) => ({ id, entries: group, logs: logs.get(id) ?? [], status: groupStatus(group) }));
}

function ActivityGroupView({ group }: { group: ActivityGroup }) {
  const [expanded, setExpanded] = useState(group.status === 'running');
  useEffect(() => {
    if (group.status === 'running') setExpanded(true);
  }, [group.status]);
  const first = group.entries[0]!;
  const target = context(first);
  return <li className="lb-activity-group" data-status={group.status}>
    <details open={expanded} onToggle={(event) => setExpanded(event.currentTarget.open)}>
      <summary>
        <span>{operationLabels[first.operation]}{target ? `: ${target}` : ''}</span>
        <time dateTime={first.timestamp}>{first.timestamp.slice(11, 19)}</time>
        <strong>{group.status.toUpperCase()}</strong>
      </summary>
      <ol className="lb-activity-stages">
        {group.entries.map((entry) => <li key={entry.id} data-status={entry.status}>
          <time dateTime={entry.timestamp}>{entry.timestamp.slice(11, 19)}</time>
          <span>{description(entry)}</span>
        </li>)}
      </ol>
      {group.logs.length > 0 && <details className="lb-verbose-logs">
        <summary>Verbose logs</summary>
        <pre>{group.logs.map((log) => `${log.timestamp.slice(11, 19)}  [${log.stream}] ${log.text}`).join('\n')}</pre>
      </details>}
    </details>
  </li>;
}

export function BottomWorkspace({ state, actions }: { state: LoadbotState; actions: LoadbotActions }) {
  const groups = activityGroups(state);
  return <>
    <div className="lb-bottom-tabs" role="tablist" aria-label="Bottom workspace">
      <button type="button" role="tab" aria-selected={state.bottomView === 'command'} onClick={() => actions.selectBottomView('command')}>COMMAND</button>
      <button type="button" role="tab" aria-selected={state.bottomView === 'activity'} onClick={() => actions.selectBottomView('activity')}>ACTIVITY</button>
      <button type="button" role="tab" aria-selected={state.bottomView === 'terminal'} onClick={() => actions.selectBottomView('terminal')}>TERMINAL</button>
    </div>
    {state.bottomView === 'command' && <CommandPane state={state} actions={actions} />}
    {state.bottomView === 'activity' && <section className="lb-bottom-content lb-activity" role="tabpanel" aria-label="Activity">
      <h2>ACTIVITY / THIS SESSION</h2>
      {!groups.length && <p className="lb-metadata">No activity yet.</p>}
      <ol className="lb-activity-groups" aria-live="polite">
        {groups.map((group) => <ActivityGroupView group={group} key={group.id} />)}
      </ol>
    </section>}
    <TerminalPane state={state} actions={actions} visible={state.bottomView === 'terminal'} />
  </>;
}
