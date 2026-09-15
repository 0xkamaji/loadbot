import { invoke, isTauri } from '@tauri-apps/api/core';
import type { LoadbotAdapter, LoadbotProject, LoadbotShortcut } from '../loadbot/contract';

/** One query seam for tests/host composition, not a generic RPC interface. */
export type InventoryQuery = () => Promise<unknown>;
export type ProjectFolderOpen = (project: Pick<LoadbotProject, 'catalog' | 'tool'>) => Promise<unknown>;

async function nativeInventoryQuery(): Promise<unknown> {
  if (!isTauri()) throw new Error('Local inventory requires the native Loadbot application.');
  return invoke('read_loadbot_inventory');
}

async function nativeProjectFolderOpen(project: Pick<LoadbotProject, 'catalog' | 'tool'>): Promise<unknown> {
  if (!isTauri()) throw new Error('Opening project folders requires the native Loadbot application.');
  return invoke('open_loadbot_project', { catalog: project.catalog, tool: project.tool });
}

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
    async openProjectFolder(project) {
      try {
        await openProject(project);
      } catch (error: unknown) {
        throw nativeError(error, 'Could not open the project folder.');
      }
    },
  };
}
