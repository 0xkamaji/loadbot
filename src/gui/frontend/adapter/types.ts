/** Minimal menu projection of launcher::Project / ProjectEntry, not a catalog schema. */
export interface MenuProject {
  catalog: string;
  tool: string;
  entries: readonly MenuShortcut[];
}

export interface MenuShortcut {
  name: string;
  path: string;
  description?: string;
  runner?: 'direct' | 'bash' | 'sh' | 'python' | 'powershell';
  source: 'catalog' | 'personal';
  /** UI demonstration only. The Rust command contract has no input schema. */
  previewFields?: readonly PreviewField[];
}

export type PreviewField =
  | { id: string; kind: 'text'; label: string; required?: boolean; initialValue?: string; placeholder?: string }
  | { id: string; kind: 'path'; label: string; required?: boolean; initialValue?: string; sampleValue: string; pathKind: 'folder' | 'file' }
  | { id: string; kind: 'checkbox'; label: string; initialValue?: boolean };

export interface MenuAdapter {
  readonly mode: 'fixture';
  readProjects(): Promise<readonly MenuProject[]>;
}

/** Hosts may supply an overlay close action. Native desktop decorations need none. */
export interface MenuHostCallbacks {
  onClose?: () => void;
}

export const projectKey = (project: MenuProject) => JSON.stringify([project.catalog, project.tool]);
export const shortcutKey = (shortcut: MenuShortcut) => JSON.stringify([shortcut.source, shortcut.name, shortcut.path]);
