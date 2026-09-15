import { invoke, isTauri } from '@tauri-apps/api/core';
import type {
  AddCatalogInput, AddProjectInput, AddShortcutInput, CatalogIdentity, LoadbotAdapter,
  LoadbotCatalog, LoadbotProject, LoadbotShortcut, ProjectIdentity, ShortcutIdentity,
} from '../loadbot/contract';

/** One query seam for tests/host composition, not a generic RPC interface. */
export type InventoryQuery = () => Promise<unknown>;
export type ProjectFolderOpen = (project: Pick<LoadbotProject, 'catalog' | 'tool'>) => Promise<unknown>;
export interface ManagementBridge {
  readCatalogs(): Promise<unknown>;
  addCatalog(input: AddCatalogInput): Promise<unknown>;
  addProject(input: AddProjectInput): Promise<unknown>;
  addShortcut(input: AddShortcutInput): Promise<unknown>;
  syncCatalog(catalog: string): Promise<unknown>;
}

async function nativeInventoryQuery(): Promise<unknown> {
  if (!isTauri()) throw new Error('Local inventory requires the native Loadbot application.');
  return invoke('read_loadbot_inventory');
}

async function nativeProjectFolderOpen(project: Pick<LoadbotProject, 'catalog' | 'tool'>): Promise<unknown> {
  if (!isTauri()) throw new Error('Opening project folders requires the native Loadbot application.');
  return invoke('open_loadbot_project', { catalog: project.catalog, tool: project.tool });
}

function requireTauri(capability: string) {
  if (!isTauri()) throw new Error(`${capability} requires the native Loadbot application.`);
}

const nativeManagementBridge: ManagementBridge = {
  async readCatalogs() { requireTauri('Catalog management'); return invoke('read_loadbot_catalogs'); },
  async addCatalog(input) {
    requireTauri('Catalog management');
    return invoke('add_loadbot_catalog', { name: input.name, url: input.url, writable: input.writable });
  },
  async addProject(input) {
    requireTauri('Project management');
    return invoke('add_loadbot_project', {
      catalog: input.catalog, name: input.name, url: input.url, revision: input.revision,
      commit: input.commit, push: input.push,
    });
  },
  async addShortcut(input) {
    requireTauri('Shortcut management');
    return invoke('add_loadbot_shortcut', {
      catalog: input.catalog, tool: input.tool, name: input.name, path: input.path,
      description: input.description, runner: input.runner,
    });
  },
  async syncCatalog(catalog) { requireTauri('Catalog synchronization'); return invoke('sync_loadbot_catalog', { catalog }); },
};

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid inventory response: expected a record.');
  return value as Record<string, unknown>;
}
function text(value: unknown): string {
  if (typeof value !== 'string') throw new Error('Invalid inventory response: expected text.');
  return value;
}
function optionalText(value: unknown): string | undefined {
  return value == null ? undefined : text(value);
}
function boolean(value: unknown): boolean {
  if (typeof value !== 'boolean') throw new Error('Invalid native response: expected a boolean.');
  return value;
}
function entry(value: unknown): LoadbotShortcut {
  const item = record(value);
  if (item.source !== 'catalog' && item.source !== 'personal') throw new Error('Invalid inventory entry source.');
  const runner = optionalText(item.runner);
  if (runner !== undefined && !['direct', 'bash', 'sh', 'python', 'powershell'].includes(runner)) throw new Error('Invalid inventory runner.');
  return {
    name: text(item.name), path: text(item.path), source: item.source,
    description: optionalText(item.description), runner: runner as LoadbotShortcut['runner'],
  };
}

/** Validate the read projection, not native paths or Loadbot's catalog rules.
 * Strings (including repository-relative paths) are preserved, never normalized.
 */
function inventory(value: unknown): readonly LoadbotProject[] {
  if (!Array.isArray(value)) throw new Error('Invalid inventory response: expected projects.');
  return value.map((value) => {
    const project = record(value);
    if (!Array.isArray(project.entries)) throw new Error('Invalid inventory response: expected entries.');
    return { catalog: text(project.catalog), tool: text(project.tool), entries: project.entries.map(entry) };
  });
}

function catalogs(value: unknown): readonly LoadbotCatalog[] {
  if (!Array.isArray(value)) throw new Error('Invalid catalog response: expected catalogs.');
  return value.map((value) => {
    const item = record(value);
    if (!['missing', 'installed', 'mismatch'].includes(String(item.state))) throw new Error('Invalid catalog state.');
    return {
      name: text(item.name), url: text(item.url), writable: boolean(item.writable),
      state: item.state as LoadbotCatalog['state'], default: boolean(item.default),
    };
  });
}

function catalogIdentity(value: unknown): CatalogIdentity {
  const item = record(value);
  return { catalog: text(item.catalog) };
}
function projectIdentity(value: unknown): ProjectIdentity {
  const item = record(value);
  return { catalog: text(item.catalog), tool: text(item.tool) };
}
function shortcutIdentity(value: unknown): ShortcutIdentity {
  const item = record(value);
  return { ...projectIdentity(item), name: text(item.name), path: text(item.path) };
}

function nativeError(error: unknown, fallback: string): Error {
  if (error instanceof Error) return error;
  const message = typeof error === 'string' ? error
    : error && typeof error === 'object' && 'message' in error && typeof error.message === 'string' ? error.message
      : fallback;
  return new Error(message);
}

export function createTauriLoadbotAdapter(
  query: InventoryQuery = nativeInventoryQuery,
  openProject: ProjectFolderOpen = nativeProjectFolderOpen,
  management: ManagementBridge = nativeManagementBridge,
): LoadbotAdapter {
  return {
    async readInventory() {
      try {
        return inventory(await query());
      } catch (error: unknown) {
        // Tauri serializes Rust command failures as { message }, rather than Error.
        throw nativeError(error, 'Could not read local Loadbot inventory.');
      }
    },
    async readCatalogs() {
      try { return catalogs(await management.readCatalogs()); }
      catch (error: unknown) { throw nativeError(error, 'Could not read configured catalogs.'); }
    },
    async openProjectFolder(project) {
      try {
        await openProject(project);
      } catch (error: unknown) {
        throw nativeError(error, 'Could not open the project folder.');
      }
    },
    async addCatalog(input) {
      try { return catalogIdentity(await management.addCatalog(input)); }
      catch (error: unknown) { throw nativeError(error, 'Could not add the catalog.'); }
    },
    async addProject(input) {
      try { return projectIdentity(await management.addProject(input)); }
      catch (error: unknown) { throw nativeError(error, 'Could not add the project.'); }
    },
    async addShortcut(input) {
      try { return shortcutIdentity(await management.addShortcut(input)); }
      catch (error: unknown) { throw nativeError(error, 'Could not add the shortcut.'); }
    },
    async syncCatalog(catalog) {
      try { await management.syncCatalog(catalog); }
      catch (error: unknown) { throw nativeError(error, 'Could not synchronize the catalog.'); }
    },
  };
}
