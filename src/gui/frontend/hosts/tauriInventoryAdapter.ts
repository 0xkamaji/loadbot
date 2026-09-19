import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import type {
  AddCatalogInput, AddProjectInput, AddShortcutInput, CatalogIdentity, LoadbotAdapter,
  CatalogSyncActivity, CatalogSyncActivitySink, LoadbotCatalog, LoadbotProject, LoadbotRecipe,
  LoadbotInterpreterRunner, LoadbotRecipeArgument, LoadbotRunner, LoadbotShortcut,
  ProjectIdentity, ShortcutIdentity,
  RecipeShortcutInput, ShortcutHelpRequest, ShortcutHelpResult,
} from '../loadbot/contract';

/** One query seam for tests/host composition, not a generic RPC interface. */
export type InventoryQuery = () => Promise<unknown>;
export type ProjectFolderOpen = (project: Pick<LoadbotProject, 'catalog' | 'tool'>) => Promise<unknown>;
export interface ManagementBridge {
  readCatalogs(): Promise<unknown>;
  openProjectTerminal?(project: ProjectIdentity): Promise<unknown>;
  pullProject?(project: ProjectIdentity): Promise<unknown>;
  updateProject?(project: ProjectIdentity): Promise<unknown>;
  removeProject?(project: ProjectIdentity): Promise<unknown>;
  reinstallProject?(project: ProjectIdentity): Promise<unknown>;
  addCatalog(input: AddCatalogInput): Promise<unknown>;
  addProject(input: AddProjectInput): Promise<unknown>;
  addShortcut(input: AddShortcutInput): Promise<unknown>;
  addRecipeShortcut(input: RecipeShortcutInput): Promise<unknown>;
  updateRecipeShortcut(input: RecipeShortcutInput): Promise<unknown>;
  chooseProjectFile(project: ProjectIdentity): Promise<unknown>;
  chooseProjectDirectory(project: ProjectIdentity): Promise<unknown>;
  viewShortcutHelp(request: ShortcutHelpRequest): Promise<unknown>;
  deleteShortcuts(shortcuts: readonly ShortcutIdentity[]): Promise<unknown>;
  syncCatalog(catalog: string, onActivity?: CatalogSyncActivitySink): Promise<unknown>;
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
  async openProjectTerminal(project) {
    requireTauri('Opening project terminals');
    return invoke('open_loadbot_project_terminal', { ...project });
  },
  async pullProject(project) { requireTauri('Project management'); return invoke('pull_loadbot_project', { ...project }); },
  async updateProject(project) { requireTauri('Project management'); return invoke('update_loadbot_project', { ...project }); },
  async removeProject(project) { requireTauri('Project management'); return invoke('remove_loadbot_project', { ...project }); },
  async reinstallProject(project) { requireTauri('Project management'); return invoke('reinstall_loadbot_project', { ...project }); },
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
  async addRecipeShortcut(input) {
    requireTauri('Recipe shortcut management');
    return invoke('add_loadbot_recipe_shortcut', { ...input });
  },
  async updateRecipeShortcut(input) {
    requireTauri('Recipe shortcut management');
    return invoke('update_loadbot_recipe_shortcut', { ...input });
  },
  async chooseProjectFile(project) {
    requireTauri('Project file selection');
    return invoke('choose_loadbot_project_file', { catalog: project.catalog, tool: project.tool });
  },
  async chooseProjectDirectory(project) {
    requireTauri('Project folder selection');
    return invoke('choose_loadbot_project_directory', { catalog: project.catalog, tool: project.tool });
  },
  async viewShortcutHelp(request) {
    requireTauri('Shortcut help');
    return invoke('view_loadbot_shortcut_help', { request });
  },
  async deleteShortcuts(shortcuts) {
    requireTauri('Shortcut management');
    return invoke('delete_loadbot_shortcuts', { identities: shortcuts });
  },
  async syncCatalog(catalog, onActivity) {
    requireTauri('Catalog synchronization');
    const channel = new Channel<unknown>();
    channel.onmessage = (value) => onActivity?.(catalogSyncActivity(value));
    return invoke('sync_loadbot_catalog', { catalog, onActivity: channel });
  },
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
function number(value: unknown): number {
  if (typeof value !== 'number' || !Number.isInteger(value)) throw new Error('Invalid inventory response: expected an integer.');
  return value;
}
function optionalPath(value: unknown): string | undefined {
  if (value == null) return undefined;
  return text(value);
}
function shortcutHelp(value: unknown): ShortcutHelpResult {
  const item = record(value);
  if (!Array.isArray(item.commandAttempted)) throw new Error('Invalid shortcut help command.');
  const exitStatus = item.exitStatus == null ? undefined : number(item.exitStatus);
  const detectedHelpFlag = optionalText(item.detectedHelpFlag);
  if (detectedHelpFlag !== undefined && detectedHelpFlag !== '--help' && detectedHelpFlag !== '-h') {
    throw new Error('Invalid shortcut help flag.');
  }
  return {
    commandAttempted: item.commandAttempted.map(text), stdout: text(item.stdout), stderr: text(item.stderr),
    exitStatus, detectedHelpFlag,
  };
}
function recipeArgument(value: unknown): LoadbotRecipeArgument {
  const item = record(value);
  const type = text(item.type);
  if (type === 'project-path') return { type, path: text(item.path) };
  if (type === 'literal') return { type, value: text(item.value) };
  if (type === 'input') {
    const kind = text(item.kind);
    if (!['text', 'file', 'directory'].includes(kind)) throw new Error('Invalid Recipe input kind.');
    return {
      type, id: text(item.id), label: text(item.label), kind: kind as 'text' | 'file' | 'directory',
      required: boolean(item.required), default: optionalText(item.default), prefix: optionalText(item.prefix),
    };
  }
  if (type === 'switch') {
    return { type, id: text(item.id), label: text(item.label), value: text(item.value), default: boolean(item.default) };
  }
  throw new Error('Invalid Recipe argument type.');
}
function recipe(value: unknown): LoadbotRecipe {
  const item = record(value);
  const behavior = text(item.behavior);
  if (behavior !== 'run' && behavior !== 'launch') throw new Error('Invalid Recipe behavior.');
  const rawProgram = record(item.program);
  const programType = text(rawProgram.type);
  let program: LoadbotRecipe['program'];
  if (programType === 'project-file') program = { type: programType, path: text(rawProgram.path) };
  else if (programType === 'interpreter') {
    const runner = text(rawProgram.runner);
    if (!['bash', 'sh', 'python', 'powershell'].includes(runner)) throw new Error('Invalid Recipe runner.');
    program = { type: programType, runner: runner as LoadbotInterpreterRunner };
  } else if (programType === 'executable') program = { type: programType, name: text(rawProgram.name) };
  else throw new Error('Invalid Recipe program type.');
  const rawWorkingDirectory = record(item.working_directory);
  const workingType = text(rawWorkingDirectory.type);
  let working_directory: LoadbotRecipe['working_directory'];
  if (workingType === 'project-root' || workingType === 'target-parent') working_directory = { type: workingType };
  else if (workingType === 'project-relative') working_directory = { type: workingType, path: text(rawWorkingDirectory.path) };
  else throw new Error('Invalid Recipe working directory.');
  if (!Array.isArray(item.arguments)) throw new Error('Invalid Recipe arguments.');
  return {
    version: number(item.version), behavior, program, working_directory,
    arguments: item.arguments.map(recipeArgument),
  };
}
function entry(value: unknown): LoadbotShortcut {
  const item = record(value);
  if (item.source !== 'catalog' && item.source !== 'personal') throw new Error('Invalid inventory entry source.');
  const source: 'catalog' | 'personal' = item.source;
  const runner = optionalText(item.runner);
  if (runner !== undefined && !['direct', 'bash', 'sh', 'python', 'powershell'].includes(runner)) throw new Error('Invalid inventory runner.');
  const path = optionalText(item.path);
  const definition = item.recipe == null ? undefined : recipe(item.recipe);
  if ((path === undefined) === (definition === undefined)) throw new Error('Invalid inventory invocation: expected legacy path or Recipe.');
  if (definition && runner !== undefined) throw new Error('Invalid inventory invocation: Recipe must not have a legacy runner.');
  const facts = { name: text(item.name), source, description: optionalText(item.description) };
  return path !== undefined
    ? { ...facts, path, runner: runner as LoadbotRunner | undefined }
    : { ...facts, recipe: definition! };
}

/** Validate the read projection, not native paths or Loadbot's catalog rules.
 * Strings (including repository-relative paths) are preserved, never normalized.
 */
function inventory(value: unknown): readonly LoadbotProject[] {
  if (!Array.isArray(value)) throw new Error('Invalid inventory response: expected projects.');
  return value.map((value) => {
    const project = record(value);
    if (!Array.isArray(project.entries)) throw new Error('Invalid inventory response: expected entries.');
    return {
      catalog: text(project.catalog), tool: text(project.tool),
      installed: project.installed == null ? true : boolean(project.installed), entries: project.entries.map(entry),
    };
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

function catalogSyncActivity(value: unknown): CatalogSyncActivity {
  const item = record(value);
  const stage = text(item.stage);
  if (!['validating', 'repository-checked', 'updating-repository', 'current', 'updated'].includes(stage)) {
    throw new Error('Invalid catalog synchronization activity stage.');
  }
  return { stage: stage as CatalogSyncActivity['stage'], catalog: text(item.catalog), detail: optionalText(item.detail) };
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
  return { ...projectIdentity(item), name: text(item.name), path: optionalText(item.path) };
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
    async openProjectTerminal(project) {
      try { await management.openProjectTerminal?.(project); }
      catch (error: unknown) { throw nativeError(error, 'Could not open a terminal for the project.'); }
    },
    async pullProject(project) {
      try { return projectIdentity(await management.pullProject?.(project)); }
      catch (error: unknown) { throw nativeError(error, 'Could not pull the project.'); }
    },
    async updateProject(project) {
      try { return projectIdentity(await management.updateProject?.(project)); }
      catch (error: unknown) { throw nativeError(error, 'Could not update the project.'); }
    },
    async removeProject(project) {
      try { return projectIdentity(await management.removeProject?.(project)); }
      catch (error: unknown) { throw nativeError(error, 'Could not remove the project.'); }
    },
    async reinstallProject(project) {
      try { return projectIdentity(await management.reinstallProject?.(project)); }
      catch (error: unknown) { throw nativeError(error, 'Could not reinstall the project.'); }
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
    async addRecipeShortcut(input) {
      try { return shortcutIdentity(await management.addRecipeShortcut(input)); }
      catch (error: unknown) { throw nativeError(error, 'Could not add the Recipe shortcut.'); }
    },
    async updateRecipeShortcut(input) {
      try { return shortcutIdentity(await management.updateRecipeShortcut(input)); }
      catch (error: unknown) { throw nativeError(error, 'Could not update the Recipe shortcut.'); }
    },
    async chooseProjectFile(project) {
      try { return optionalPath(await management.chooseProjectFile(project)); }
      catch (error: unknown) { throw nativeError(error, 'Could not choose a file in the project.'); }
    },
    async chooseProjectDirectory(project) {
      try { return optionalPath(await management.chooseProjectDirectory(project)); }
      catch (error: unknown) { throw nativeError(error, 'Could not choose a folder in the project.'); }
    },
    async viewShortcutHelp(request) {
      try { return shortcutHelp(await management.viewShortcutHelp(request)); }
      catch (error: unknown) { throw nativeError(error, 'Could not view help for this target.'); }
    },
    async deleteShortcuts(shortcuts) {
      try { return number(await management.deleteShortcuts(shortcuts)); }
      catch (error: unknown) { throw nativeError(error, 'Could not delete the selected shortcuts.'); }
    },
    async syncCatalog(catalog, onActivity) {
      try { await management.syncCatalog(catalog, onActivity); }
      catch (error: unknown) { throw nativeError(error, 'Could not synchronize the catalog.'); }
    },
  };
}
