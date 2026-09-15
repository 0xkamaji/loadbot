import type {
  AddCatalogInput, AddProjectInput, AddShortcutInput, LoadbotAdapter, LoadbotCatalog,
  LoadbotProject, LoadbotShortcut,
} from '../contract';
import { projectKey, selectionKey, shortcutKey } from '../identity';
import { initialValues, missingInputs, noSampleForms, type SampleField, type SampleForms, type SampleValues } from './sampleForms';

export type InventoryState =
  | { readonly status: 'loading' }
  | { readonly status: 'ready'; readonly projects: readonly LoadbotProject[] }
  | { readonly status: 'error'; readonly message?: string };
export type CatalogState =
  | { readonly status: 'loading' }
  | { readonly status: 'ready'; readonly catalogs: readonly LoadbotCatalog[] }
  | { readonly status: 'error'; readonly message?: string };
export type ManagementKind = 'add-catalog' | 'add-project' | 'add-shortcut' | 'sync-catalog';
export type ManagementState =
  | { readonly status: 'idle' }
  | { readonly status: 'submitting'; readonly kind: ManagementKind; readonly message: string }
  | { readonly status: 'success'; readonly kind: ManagementKind; readonly message: string }
  | { readonly status: 'error'; readonly kind: ManagementKind; readonly message: string };

export interface LoadbotState {
  readonly inventory: InventoryState;
  readonly catalogState: CatalogState;
  readonly currentCatalog?: string;
  readonly project?: LoadbotProject;
  readonly shortcut?: LoadbotShortcut;
  readonly fields: readonly SampleField[];
  readonly values: SampleValues;
  readonly missingInputIds: readonly string[];
  readonly drawerOpen: boolean;
  readonly management: ManagementState;
  readonly projectFolder: {
    readonly status: 'idle' | 'opening' | 'opened' | 'error';
    readonly projectId?: string;
    readonly message?: string;
  };
}

export interface LoadbotActions {
  selectCatalog(name: string): void;
  selectProject(id: string): void;
  selectShortcut(id: string): void;
  reloadInventory(): void;
  openProjectFolder(id: string): void;
  addCatalog(input: AddCatalogInput): Promise<boolean>;
  addProject(input: Omit<AddProjectInput, 'catalog'>): Promise<boolean>;
  addShortcut(input: Omit<AddShortcutInput, 'catalog' | 'tool'>): Promise<boolean>;
  syncCatalog(): Promise<boolean>;
  clearManagementStatus(): void;
  changeSampleInput(id: string, value: string | boolean): void;
  useSamplePath(id: string): void;
  toggleDrawer(): void;
}

interface ReadPreference { catalog?: string; projectId?: string; shortcutId?: string }

/** Deterministic application state, independent of React, DOM, themes and hosts.
 * Backend-confirmed writes are always followed by an authoritative workspace read.
 */
export function createLoadbotApplication(adapter: LoadbotAdapter, sampleForms: SampleForms = noSampleForms) {
  let state: LoadbotState = {
    inventory: { status: 'loading' }, catalogState: { status: 'loading' }, fields: [], values: {},
    missingInputIds: [], drawerOpen: true, management: { status: 'idle' },
    projectFolder: { status: 'idle' },
  };
  const listeners = new Set<() => void>();
  let generation = 0;
  let folderGeneration = 0;
  const publish = (next: LoadbotState) => {
    state = next;
    listeners.forEach((listener) => listener());
  };
  const selection = (project?: LoadbotProject, shortcut = project?.entries[0]) => {
    const fields = project && shortcut ? sampleForms[selectionKey(project, shortcut)] ?? [] : [];
    const values = initialValues(fields);
    return { project, shortcut, fields, values, missingInputIds: missingInputs(fields, values) };
  };
  const projectsFor = (projects: readonly LoadbotProject[], catalog?: string) =>
    catalog ? projects.filter((project) => project.catalog === catalog) : projects;

  function changeSampleInput(id: string, value: string | boolean) {
    const field = state.fields.find((item) => item.id === id);
    if (!field || (field.kind === 'boolean' ? typeof value !== 'boolean' : typeof value !== 'string')) return;
    const values = { ...state.values, [id]: value };
    publish({ ...state, values, missingInputIds: missingInputs(state.fields, values) });
  }

  async function beginWorkspaceRead(preferred: ReadPreference = {}): Promise<boolean> {
    const request = ++generation;
    const selectedProject = preferred.projectId ?? (state.project && projectKey(state.project));
    const selectedShortcut = preferred.shortcutId ?? (state.shortcut && shortcutKey(state.shortcut));
    const selectedCatalog = preferred.catalog ?? state.currentCatalog;
    folderGeneration++;
    publish({ ...state, inventory: { status: 'loading' }, catalogState: { status: 'loading' }, ...selection(), projectFolder: { status: 'idle' } });
    try {
      const [projects, catalogs] = await Promise.all([adapter.readInventory(), adapter.readCatalogs()]);
      if (request !== generation) return false;
      const catalogNames = new Set(catalogs.map((catalog) => catalog.name));
      const currentCatalog = selectedCatalog && (catalogNames.has(selectedCatalog) || projects.some((project) => project.catalog === selectedCatalog))
        ? selectedCatalog
        : catalogs.find((catalog) => catalog.default)?.name ?? projects[0]?.catalog ?? catalogs[0]?.name;
      const visible = projectsFor(projects, currentCatalog);
      const project = visible.find((item) => projectKey(item) === selectedProject) ?? visible[0];
      const shortcut = project?.entries.find((item) => shortcutKey(item) === selectedShortcut);
      publish({
        ...state, inventory: { status: 'ready', projects }, catalogState: { status: 'ready', catalogs }, currentCatalog,
        ...selection(project, shortcut),
      });
      return true;
    } catch (error: unknown) {
      if (request === generation) publish({
        ...state,
        inventory: { status: 'error', message: error instanceof Error ? error.message : undefined },
        catalogState: { status: 'error', message: error instanceof Error ? error.message : undefined },
        ...selection(), projectFolder: { status: 'idle' },
      });
      return false;
    }
  }

  async function mutation(kind: ManagementKind, progress: string, completed: string, operation: () => Promise<ReadPreference>): Promise<boolean> {
    if (state.management.status === 'submitting') return false;
    publish({ ...state, management: { status: 'submitting', kind, message: progress } });
    try {
      const preferred = await operation();
      const reloaded = await beginWorkspaceRead(preferred);
      publish({
        ...state,
        management: reloaded
          ? { status: 'success', kind, message: completed }
          : { status: 'error', kind, message: `${completed} Local state could not be reread; use RELOAD LOCAL.` },
      });
      return true;
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : 'The Loadbot operation failed.';
      // Some shared operations can report a failure after an earlier durable step
      // (for example, a catalog definition saved before an explicit push fails).
      // Never guess: reread local authority before presenting the failure.
      await beginWorkspaceRead();
      publish({
        ...state,
        management: { status: 'error', kind, message },
      });
      return false;
    }
  }

  const actions: LoadbotActions = {
    selectCatalog(name) {
      if (state.catalogState.status !== 'ready' || !state.catalogState.catalogs.some((item) => item.name === name)) return;
      const projects = state.inventory.status === 'ready' ? projectsFor(state.inventory.projects, name) : [];
      folderGeneration++;
      publish({ ...state, currentCatalog: name, ...selection(projects[0]), projectFolder: { status: 'idle' } });
    },
    selectProject(id) {
      if (state.inventory.status !== 'ready') return;
      const project = projectsFor(state.inventory.projects, state.currentCatalog).find((item) => projectKey(item) === id);
      if (!project || project === state.project) return;
      folderGeneration++;
      publish({ ...state, ...selection(project), projectFolder: { status: 'idle' } });
    },
    selectShortcut(id) {
      const shortcut = state.project?.entries.find((item) => shortcutKey(item) === id);
      if (!shortcut || shortcut === state.shortcut) return;
      publish({ ...state, ...selection(state.project, shortcut) });
    },
    reloadInventory() { if (state.management.status !== 'submitting') void beginWorkspaceRead(); },
    openProjectFolder(id) {
      if (state.inventory.status !== 'ready') return;
      const project = projectsFor(state.inventory.projects, state.currentCatalog).find((item) => projectKey(item) === id);
      if (!project) return;
      const request = ++folderGeneration;
      publish({ ...state, projectFolder: { status: 'opening', projectId: id } });
      const identity = { catalog: project.catalog, tool: project.tool };
      Promise.resolve().then(() => adapter.openProjectFolder(identity)).then(
        () => {
          if (request === folderGeneration) publish({ ...state, projectFolder: { status: 'opened', projectId: id, message: `Opened ${project.tool}.` } });
        },
        (error: unknown) => {
          if (request === folderGeneration) publish({
            ...state,
            projectFolder: { status: 'error', projectId: id, message: error instanceof Error ? error.message : 'Could not open the project folder.' },
          });
        },
      );
    },
    async addCatalog(input) {
      return mutation('add-catalog', `Adding catalog ${input.name}…`, `Catalog ${input.name} added.`, async () => {
        const created = await adapter.addCatalog(input);
        return { catalog: created.catalog };
      });
    },
    async addProject(input) {
      const catalog = state.currentCatalog;
      if (!catalog) return false;
      return mutation('add-project', `Adding project ${input.name}…`, `Project ${input.name} added.`, async () => {
        const created = await adapter.addProject({ ...input, catalog });
        return { catalog: created.catalog, projectId: projectKey(created) };
      });
    },
    async addShortcut(input) {
      const project = state.project;
      if (!project) return false;
      return mutation('add-shortcut', `Adding shortcut ${input.name}…`, `Shortcut ${input.name} added.`, async () => {
        const created = await adapter.addShortcut({ ...input, catalog: project.catalog, tool: project.tool });
        return { catalog: created.catalog, projectId: projectKey(created), shortcutId: JSON.stringify(['personal', created.name, created.path]) };
      });
    },
    async syncCatalog() {
      const catalog = state.currentCatalog;
      if (!catalog) return false;
      return mutation('sync-catalog', `Synchronizing ${catalog}…`, `Catalog ${catalog} synchronized.`, async () => {
        await adapter.syncCatalog(catalog);
        return { catalog };
      });
    },
    clearManagementStatus() { if (state.management.status !== 'submitting') publish({ ...state, management: { status: 'idle' } }); },
    changeSampleInput,
    useSamplePath(id) {
      const field = state.fields.find((item) => item.id === id);
      if (field?.kind === 'path') changeSampleInput(id, field.sampleValue);
    },
    toggleDrawer() { publish({ ...state, drawerOpen: !state.drawerOpen }); },
  };

  return {
    getSnapshot: () => state,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    actions,
    start() {
      const request = generation + 1;
      void beginWorkspaceRead();
      return () => { if (request === generation) generation++; };
    },
  };
}
