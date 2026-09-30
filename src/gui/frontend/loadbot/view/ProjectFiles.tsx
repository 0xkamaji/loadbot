import type { LoadbotActions, LoadbotState } from '../application/controller';
import { Button, StatusDisplay } from '../../ui/components';

function fileSize(bytes: number | undefined): string {
  if (bytes === undefined) return '';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function ProjectFiles({ state, actions }: { state: LoadbotState; actions: LoadbotActions }) {
  const project = state.project;
  const files = state.projectFiles;
  if (!project) return <StatusDisplay>Select a project to browse its files.</StatusDisplay>;
  if (project.installed === false) return <StatusDisplay>Install this project before browsing its files.</StatusDisplay>;
  if (files.status === 'idle') return <StatusDisplay>Project files have not been loaded yet.</StatusDisplay>;
  if (files.status === 'loading') return <StatusDisplay>Reading project files…</StatusDisplay>;
  if (files.status === 'error') return <div className="lb-files-status">
    <StatusDisplay>{files.message ?? 'Could not read project files.'}</StatusDisplay>
    <Button onClick={() => { void actions.readProjectDirectory(files.relativePath); }}>RETRY</Button>
  </div>;
  const listing = files.listing!;
  return <div className="lb-files-browser">
    <div className="lb-files-path">
      {listing.parent !== undefined && <Button className="lb-files-up" aria-label="Go to parent directory"
        title="Go to parent directory" onClick={() => { void actions.readProjectDirectory(listing.parent); }}>UP</Button>}
      <span title={listing.path || 'Project root'}>{listing.path || 'PROJECT ROOT'}</span>
      <Button onClick={() => { void actions.readProjectDirectory(listing.path); }}>REFRESH</Button>
    </div>
    <div className="lb-files-list" role="list" aria-label={`Files in ${listing.path || 'project root'}`}>
      {listing.entries.map((entry) => entry.kind === 'directory'
        ? <div role="listitem" key={entry.path}><button type="button" className="lb-file-entry lb-directory-entry"
          onClick={() => { void actions.readProjectDirectory(entry.path); }}>
          <span className="lb-file-kind">DIR</span><strong>{entry.name}</strong><small>Open folder</small>
        </button></div>
        : <div role="listitem" className="lb-file-entry" key={entry.path}>
          <span className="lb-file-kind">FILE</span><strong>{entry.name}</strong><small>{fileSize(entry.size)}</small>
        </div>)}
      {!listing.entries.length && <StatusDisplay>This directory is empty.</StatusDisplay>}
    </div>
  </div>;
}
