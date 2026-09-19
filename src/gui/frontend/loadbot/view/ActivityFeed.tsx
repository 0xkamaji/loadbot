import type { ActivityEntry, LoadbotActions, LoadbotState } from '../application/controller';
import { CommandPane } from './CommandPane';

const operationLabels: Record<ActivityEntry['operation'], string> = {
  'catalog-sync': 'Refresh catalog',
  'catalog-add': 'Add catalog',
  'project-add': 'Add project',
  'shortcut-add': 'Add shortcut',
  'shortcut-update': 'Update shortcut',
  'shortcut-delete': 'Delete shortcut',
  'local-reload': 'Reload',
  'project-folder-open': 'Open project folder',
  'project-terminal-open': 'Open project terminal',
  'project-pull': 'Pull project',
  'project-update': 'Update project',
  'project-remove': 'Remove project',
  'project-reinstall': 'Reinstall project',
};

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
    case 'cloning-project': return `${entry.operation === 'project-reinstall' ? 'Cloning fresh checkout' : 'Cloning project'}: ${target}`;
    case 'validating-fresh-checkout': return `Validating fresh checkout: ${target}`;
    case 'fetching-and-updating': return `Fetching remote and updating checkout: ${target}`;
    case 'removing-checkout': return `Removing local checkout: ${target}`;
    case 'replacing-checkout': return `Replacing checkout: ${target}`;
    case 'authoritative-reload': return 'Rereading authoritative local inventory and catalog state';
    case 'catalog-state': return `Catalog state: ${entry.catalog} · ${entry.detail}`;
    case 'completed': return entry.detail ?? `${operationLabels[entry.operation]} completed`;
    case 'failed': return `${operationLabels[entry.operation]} failed${entry.detail ? `: ${entry.detail}` : ''}`;
    default: return entry.detail ?? entry.stage;
  }
}

export function BottomWorkspace({ state, actions }: { state: LoadbotState; actions: LoadbotActions }) {
  return <>
    <div className="lb-bottom-tabs" role="tablist" aria-label="Bottom workspace">
      <button type="button" role="tab" aria-selected={state.bottomView === 'command'} onClick={() => actions.selectBottomView('command')}>COMMAND</button>
      <button type="button" role="tab" aria-selected={state.bottomView === 'activity'} onClick={() => actions.selectBottomView('activity')}>ACTIVITY</button>
    </div>
    {state.bottomView === 'command' ? <CommandPane state={state} actions={actions} /> : <section className="lb-bottom-content lb-activity" role="tabpanel" aria-label="Activity">
      <h2>ACTIVITY / THIS SESSION</h2>
      {!state.activity.length && <p className="lb-metadata">No activity yet.</p>}
      <ol aria-live="polite">
        {state.activity.map((entry) => <li key={entry.id} data-status={entry.status}>
          <time dateTime={entry.timestamp}>{entry.timestamp.slice(11, 19)}</time>
          <span>{description(entry)}</span>
        </li>)}
      </ol>
    </section>}
  </>;
}
