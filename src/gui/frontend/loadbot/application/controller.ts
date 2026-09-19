import type {
  AddCatalogInput, AddProjectInput, AddShortcutInput, CatalogSyncActivity, CatalogSyncStage, LoadbotAdapter, LoadbotCatalog,
  LoadbotProject, LoadbotShortcut,
  LoadbotRecipe, LoadbotRecipeArgument, LoadbotRunner, ShortcutIdentity,
} from '../contract';
import { projectKey, selectionKey, shortcutKey } from '../identity';
import { completeLoadbotCommand, executeLoadbotCommand, type CommandCompletion, type CommandResult } from './command';
import { initialValues, missingInputs, noSampleForms, type SampleField, type SampleForms, type SampleValues } from './sampleForms';
import {
  addDraftArgument, draftValidation, moveDraftArgument, newRecipeDraft, recipeDraftFromShortcut,
  recipeFromDraft, removeDraftArgument, updateDraftArgument, updateDraftRunner, updateDraftTarget,
  type RecipeDraft, type RecipeParameterKind,
} from './recipeEditor';

export type InventoryState =
  | { readonly status: 'loading' }
  | { readonly status: 'ready'; readonly projects: readonly LoadbotProject[] }
  | { readonly status: 'error'; readonly message?: string };
export type CatalogState =
  | { readonly status: 'loading' }
  | { readonly status: 'ready'; readonly catalogs: readonly LoadbotCatalog[] }
  | { readonly status: 'error'; readonly message?: string };
export type ManagementKind = 'add-catalog' | 'add-project' | 'add-shortcut' | 'update-shortcut' | 'delete-shortcut' | 'sync-catalog';
export type ManagementState =
  | { readonly status: 'idle' }
  | { readonly status: 'submitting'; readonly kind: ManagementKind; readonly message: string }
  | { readonly status: 'success'; readonly kind: ManagementKind; readonly message: string }
  | { readonly status: 'error'; readonly kind: ManagementKind; readonly message: string };
export type ActivityOperation = 'catalog-sync' | 'catalog-add' | 'project-add' | 'shortcut-add' | 'shortcut-update' | 'shortcut-delete' | 'local-reload' | 'project-folder-open';
export type ActivityStatus = 'in-progress' | 'info' | 'success' | 'error';
export type ActivityStage = CatalogSyncStage | 'started' | 'authoritative-reload' | 'catalog-state' | 'completed' | 'failed';
export interface ActivityEntry {
  readonly id: number;
  readonly timestamp: string;
  readonly operation: ActivityOperation;
  readonly stage: ActivityStage;
  readonly status: ActivityStatus;
  readonly catalog?: string;
  readonly project?: string;
  readonly shortcut?: string;
  readonly detail?: string;
}
export const ACTIVITY_HISTORY_LIMIT = 250;
export const COMMAND_HISTORY_LIMIT = 100;
export interface CommandEntry {
  readonly id: number;
  readonly input: string;
  readonly result: CommandResult;
}
export interface CommandState {
  readonly entries: readonly CommandEntry[];
  readonly history: readonly string[];
}

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
  readonly bottomView: 'command' | 'activity';
  readonly command: CommandState;
  readonly activity: readonly ActivityEntry[];
  readonly management: ManagementState;
  readonly recipeEditor?: { readonly draft: RecipeDraft; readonly errors: readonly string[] };
  readonly shortcutManagement: {
    readonly active: boolean;
    readonly selected: readonly string[];
    readonly pendingDelete?: readonly ShortcutIdentity[];
  };
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
  openRecipeCreator(): void;
  openSelectedRecipeEditor(): boolean;
  closeRecipeEditor(): void;
  updateRecipeDetails(values: Partial<Pick<RecipeDraft, 'name' | 'description'>>): void;
  setRecipeTarget(target: string): void;
  setRecipeRunner(runner: LoadbotRunner): void;
  setRecipeWorkingDirectory(workingDirectory: LoadbotRecipe['working_directory']): void;
  addRecipeParameter(kind: RecipeParameterKind): void;
  updateRecipeParameter(key: number, value: LoadbotRecipeArgument, idManuallyEdited?: boolean): void;
  removeRecipeParameter(key: number): void;
  moveRecipeParameter(key: number, direction: -1 | 1): void;
  chooseRecipeTarget(): Promise<boolean>;
  chooseRecipeArgumentPath(key: number): Promise<boolean>;
  chooseRecipeWorkingDirectory(): Promise<boolean>;
  saveRecipe(): Promise<boolean>;
  enterShortcutManagement(): void;
  exitShortcutManagement(): void;
  toggleShortcutForDeletion(id: string): void;
  requestSelectedShortcutDeletion(): void;
  requestCurrentShortcutDeletion(): void;
  cancelShortcutDeletion(): void;
  confirmShortcutDeletion(): Promise<boolean>;
  syncCatalog(): Promise<boolean>;
  clearManagementStatus(): void;
  selectBottomView(view: 'command' | 'activity'): void;
  completeCommand(input: string, caret: number): CommandCompletion | undefined;
  submitCommand(input: string): boolean;
  changeSampleInput(id: string, value: string | boolean): void;
  useSamplePath(id: string): void;
  toggleDrawer(): void;
}

interface ReadPreference { catalog?: string; projectId?: string; shortcutId?: string; shortcutName?: string }

/** Deterministic application state, independent of React, DOM, themes and hosts.
 * Backend-confirmed writes are always followed by an authoritative workspace read.
 */
export function createLoadbotApplication(adapter: LoadbotAdapter, sampleForms: SampleForms = noSampleForms) {
  let state: LoadbotState = {
    inventory: { status: 'loading' }, catalogState: { status: 'loading' }, fields: [], values: {},
    missingInputIds: [], drawerOpen: true, bottomView: 'command', command: { entries: [], history: [] },
    activity: [], management: { status: 'idle' }, shortcutManagement: { active: false, selected: [] },
    projectFolder: { status: 'idle' },
  };
  const listeners = new Set<() => void>();
  let generation = 0;
  let folderGeneration = 0;
  let activityId = 0;
  let commandId = 0;
  let recipeArgumentKey = 1000;
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
  const appendActivity = (entry: Omit<ActivityEntry, 'id' | 'timestamp'>, reveal = true) => {
    const activity = [...state.activity, { ...entry, id: ++activityId, timestamp: new Date().toISOString() }]
      .slice(-ACTIVITY_HISTORY_LIMIT);
    publish({ ...state, activity, bottomView: reveal ? 'activity' : state.bottomView });
  };
  const errorMessage = (error: unknown, fallback: string) => error instanceof Error ? error.message : fallback;

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
      const shortcut = project?.entries.find((item) => shortcutKey(item) === selectedShortcut)
        ?? project?.entries.find((item) => item.source === 'personal' && item.name === preferred.shortcutName);
      const deletable = new Set(project?.entries.filter((item) => item.source === 'personal').map(shortcutKey) ?? []);
      const shortcutManagement = {
        active: state.shortcutManagement.active,
        selected: state.shortcutManagement.selected.filter((id) => deletable.has(id)),
      };
      publish({
        ...state, inventory: { status: 'ready', projects }, catalogState: { status: 'ready', catalogs }, currentCatalog,
        ...selection(project, shortcut), shortcutManagement,
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

  async function mutation(
    kind: ManagementKind,
    activityOperation: ActivityOperation,
    progress: string,
    completed: string,
    context: Pick<ActivityEntry, 'catalog' | 'project' | 'shortcut'>,
    operation: () => Promise<ReadPreference>,
  ): Promise<boolean> {
    if (state.management.status === 'submitting') return false;
    publish({ ...state, management: { status: 'submitting', kind, message: progress } });
    appendActivity({ operation: activityOperation, stage: 'started', status: 'in-progress', ...context });
    try {
      const preferred = await operation();
      appendActivity({ operation: activityOperation, stage: 'authoritative-reload', status: 'in-progress', ...context });
      const reloaded = await beginWorkspaceRead(preferred);
      if (reloaded && activityOperation === 'catalog-sync' && state.catalogState.status === 'ready') {
        const catalog = state.catalogState.catalogs.find((item) => item.name === preferred.catalog);
        if (catalog) appendActivity({
          operation: activityOperation, stage: 'catalog-state', status: 'info', catalog: catalog.name,
          detail: `${catalog.state} · ${catalog.writable ? 'writable' : 'read-only'}`,
        });
      }
      publish({
        ...state,
        management: reloaded
          ? { status: 'success', kind, message: completed }
          : { status: 'error', kind, message: `${completed} Local state could not be reread; use RELOAD LOCAL.` },
      });
      appendActivity({
        operation: activityOperation, stage: reloaded ? 'completed' : 'failed', status: reloaded ? 'success' : 'error',
        ...context, detail: reloaded ? completed : 'The operation completed, but local state could not be reread.',
      });
      return true;
    } catch (error: unknown) {
      const message = errorMessage(error, 'The Loadbot operation failed.');
      // Some shared operations can report a failure after an earlier durable step
      // (for example, a catalog definition saved before an explicit push fails).
      // Never guess: reread local authority before presenting the failure.
      appendActivity({ operation: activityOperation, stage: 'authoritative-reload', status: 'in-progress', ...context });
      await beginWorkspaceRead();
      publish({
        ...state,
        management: { status: 'error', kind, message },
      });
      appendActivity({ operation: activityOperation, stage: 'failed', status: 'error', ...context, detail: message });
      return false;
    }
  }

  const actions: LoadbotActions = {
    selectCatalog(name) {
      if (state.catalogState.status !== 'ready' || !state.catalogState.catalogs.some((item) => item.name === name)) return;
      const projects = state.inventory.status === 'ready' ? projectsFor(state.inventory.projects, name) : [];
      folderGeneration++;
      publish({ ...state, currentCatalog: name, ...selection(projects[0]), projectFolder: { status: 'idle' }, shortcutManagement: { active: false, selected: [] } });
    },
    selectProject(id) {
      if (state.inventory.status !== 'ready') return;
      const project = projectsFor(state.inventory.projects, state.currentCatalog).find((item) => projectKey(item) === id);
      if (!project || project === state.project) return;
      folderGeneration++;
      publish({ ...state, ...selection(project), projectFolder: { status: 'idle' }, shortcutManagement: { active: state.shortcutManagement.active, selected: [] } });
    },
    selectShortcut(id) {
      const shortcut = state.project?.entries.find((item) => shortcutKey(item) === id);
      if (!shortcut || shortcut === state.shortcut) return;
      publish({ ...state, ...selection(state.project, shortcut) });
    },
    reloadInventory() {
      if (state.management.status === 'submitting') return;
      appendActivity({ operation: 'local-reload', stage: 'started', status: 'in-progress' });
      void beginWorkspaceRead().then((reloaded) => appendActivity({
        operation: 'local-reload', stage: reloaded ? 'completed' : 'failed', status: reloaded ? 'success' : 'error',
        detail: reloaded ? 'Local inventory and catalog context reread.' : 'Could not reread local Loadbot state.',
      }));
    },
    openProjectFolder(id) {
      if (state.inventory.status !== 'ready') return;
      const project = projectsFor(state.inventory.projects, state.currentCatalog).find((item) => projectKey(item) === id);
      if (!project) return;
      const request = ++folderGeneration;
      publish({ ...state, projectFolder: { status: 'opening', projectId: id } });
      const identity = { catalog: project.catalog, tool: project.tool };
      appendActivity({ operation: 'project-folder-open', stage: 'started', status: 'in-progress', catalog: project.catalog, project: project.tool });
      Promise.resolve().then(() => adapter.openProjectFolder(identity)).then(
        () => {
          if (request === folderGeneration) {
            publish({ ...state, projectFolder: { status: 'opened', projectId: id, message: `Opened ${project.tool}.` } });
            appendActivity({ operation: 'project-folder-open', stage: 'completed', status: 'success', catalog: project.catalog, project: project.tool });
          }
        },
        (error: unknown) => {
          if (request === folderGeneration) {
            const message = errorMessage(error, 'Could not open the project folder.');
            publish({ ...state, projectFolder: { status: 'error', projectId: id, message } });
            appendActivity({ operation: 'project-folder-open', stage: 'failed', status: 'error', catalog: project.catalog, project: project.tool, detail: message });
          }
        },
      );
    },
    async addCatalog(input) {
      return mutation('add-catalog', 'catalog-add', `Adding catalog ${input.name}…`, `Catalog ${input.name} added.`, { catalog: input.name }, async () => {
        const created = await adapter.addCatalog(input);
        return { catalog: created.catalog };
      });
    },
    async addProject(input) {
      const catalog = state.currentCatalog;
      if (!catalog) return false;
      return mutation('add-project', 'project-add', `Adding project ${input.name}…`, `Project ${input.name} added.`, { catalog, project: input.name }, async () => {
        const created = await adapter.addProject({ ...input, catalog });
        return { catalog: created.catalog, projectId: projectKey(created) };
      });
    },
    async addShortcut(input) {
      const project = state.project;
      if (!project) return false;
      return mutation('add-shortcut', 'shortcut-add', `Adding shortcut ${input.name}…`, `Shortcut ${input.name} added.`, { catalog: project.catalog, project: project.tool, shortcut: input.name }, async () => {
        const created = await adapter.addShortcut({ ...input, catalog: project.catalog, tool: project.tool });
        return { catalog: created.catalog, projectId: projectKey(created), shortcutName: created.name };
      });
    },
    openRecipeCreator() {
      publish({ ...state, recipeEditor: { draft: newRecipeDraft(), errors: [] } });
    },
    openSelectedRecipeEditor() {
      const draft = state.shortcut && recipeDraftFromShortcut(state.shortcut);
      if (!draft) return false;
      publish({ ...state, recipeEditor: { draft, errors: [] }, management: { status: 'idle' } });
      return true;
    },
    closeRecipeEditor() {
      if (state.management.status !== 'submitting') publish({ ...state, recipeEditor: undefined });
    },
    updateRecipeDetails(values) {
      if (!state.recipeEditor) return;
      publish({ ...state, recipeEditor: { draft: { ...state.recipeEditor.draft, ...values }, errors: [] } });
    },
    setRecipeTarget(target) {
      if (!state.recipeEditor) return;
      publish({ ...state, recipeEditor: { draft: updateDraftTarget(state.recipeEditor.draft, target), errors: [] } });
    },
    setRecipeRunner(runner) {
      if (!state.recipeEditor) return;
      publish({ ...state, recipeEditor: { draft: updateDraftRunner(state.recipeEditor.draft, runner), errors: [] } });
    },
    setRecipeWorkingDirectory(working_directory) {
      if (!state.recipeEditor) return;
      publish({ ...state, recipeEditor: { draft: { ...state.recipeEditor.draft, workingDirectory: working_directory }, errors: [] } });
    },
    addRecipeParameter(kind) {
      if (!state.recipeEditor) return;
      publish({ ...state, recipeEditor: { draft: addDraftArgument(state.recipeEditor.draft, kind, ++recipeArgumentKey), errors: [] } });
    },
    updateRecipeParameter(key, value, idManuallyEdited) {
      if (!state.recipeEditor) return;
      publish({ ...state, recipeEditor: { draft: updateDraftArgument(state.recipeEditor.draft, key, value, idManuallyEdited), errors: [] } });
    },
    removeRecipeParameter(key) {
      if (!state.recipeEditor) return;
      publish({ ...state, recipeEditor: { draft: removeDraftArgument(state.recipeEditor.draft, key), errors: [] } });
    },
    moveRecipeParameter(key, direction) {
      if (!state.recipeEditor) return;
      publish({ ...state, recipeEditor: { draft: moveDraftArgument(state.recipeEditor.draft, key, direction), errors: [] } });
    },
    async chooseRecipeTarget() {
      const editor = state.recipeEditor;
      const project = state.project;
      if (!editor || !project) return false;
      try {
        const path = await adapter.chooseProjectFile({ catalog: project.catalog, tool: project.tool });
        if (path === undefined) return false;
        actions.setRecipeTarget(path);
        return true;
      } catch (error: unknown) {
        publish({ ...state, recipeEditor: { ...editor, errors: [errorMessage(error, 'Could not choose a project file.')] } });
        return false;
      }
    },
    async chooseRecipeArgumentPath(key) {
      const editor = state.recipeEditor;
      const project = state.project;
      const argument = editor?.draft.arguments.find((item) => item.key === key);
      if (!editor || !project || argument?.value.type !== 'project-path') return false;
      try {
        const path = await adapter.chooseProjectFile({ catalog: project.catalog, tool: project.tool });
        if (path === undefined) return false;
        actions.updateRecipeParameter(key, { type: 'project-path', path });
        return true;
      } catch (error: unknown) {
        publish({ ...state, recipeEditor: { ...editor, errors: [errorMessage(error, 'Could not choose a project file.')] } });
        return false;
      }
    },
    async chooseRecipeWorkingDirectory() {
      const editor = state.recipeEditor;
      const project = state.project;
      if (!editor || !project) return false;
      try {
        const path = await adapter.chooseProjectDirectory({ catalog: project.catalog, tool: project.tool });
        if (path === undefined) return false;
        actions.setRecipeWorkingDirectory({ type: 'project-relative', path });
        return true;
      } catch (error: unknown) {
        publish({ ...state, recipeEditor: { ...editor, errors: [errorMessage(error, 'Could not choose a project folder.')] } });
        return false;
      }
    },
    async saveRecipe() {
      const editor = state.recipeEditor;
      const project = state.project;
      if (!editor || !project) return false;
      const errors = draftValidation(editor.draft);
      if (errors.length) {
        publish({ ...state, recipeEditor: { ...editor, errors } });
        return false;
      }
      const input = {
        catalog: project.catalog, tool: project.tool, name: editor.draft.name.trim(),
        description: editor.draft.description.trim() || undefined, recipe: recipeFromDraft(editor.draft),
      };
      const kind = editor.draft.mode === 'create' ? 'add-shortcut' : 'update-shortcut';
      const operation = editor.draft.mode === 'create' ? 'shortcut-add' : 'shortcut-update';
      const verb = editor.draft.mode === 'create' ? 'Creating' : 'Updating';
      const completed = editor.draft.mode === 'create' ? 'created' : 'updated';
      const result = await mutation(kind, operation, `${verb} shortcut ${input.name}…`, `Shortcut ${input.name} ${completed}.`, {
        catalog: project.catalog, project: project.tool, shortcut: input.name,
      }, async () => {
        const saved = editor.draft.mode === 'create'
          ? await adapter.addRecipeShortcut(input) : await adapter.updateRecipeShortcut(input);
        return { catalog: saved.catalog, projectId: projectKey(saved), shortcutName: saved.name };
      });
      if (result) publish({ ...state, recipeEditor: undefined });
      return result;
    },
    enterShortcutManagement() {
      publish({ ...state, shortcutManagement: { active: true, selected: [] } });
    },
    exitShortcutManagement() {
      if (state.management.status !== 'submitting') publish({ ...state, shortcutManagement: { active: false, selected: [] } });
    },
    toggleShortcutForDeletion(id) {
      if (!state.shortcutManagement.active || !state.project) return;
      const shortcut = state.project.entries.find((item) => shortcutKey(item) === id);
      if (shortcut?.source !== 'personal') return;
      const selected = state.shortcutManagement.selected.includes(id)
        ? state.shortcutManagement.selected.filter((item) => item !== id)
        : [...state.shortcutManagement.selected, id];
      publish({ ...state, shortcutManagement: { ...state.shortcutManagement, selected } });
    },
    requestSelectedShortcutDeletion() {
      const project = state.project;
      if (!project || !state.shortcutManagement.selected.length) return;
      const selected = new Set(state.shortcutManagement.selected);
      const pendingDelete = project.entries.filter((item) => item.source === 'personal' && selected.has(shortcutKey(item)))
        .map((item) => ({ catalog: project.catalog, tool: project.tool, name: item.name, path: item.path }));
      if (pendingDelete.length) publish({ ...state, shortcutManagement: { ...state.shortcutManagement, pendingDelete } });
    },
    requestCurrentShortcutDeletion() {
      const project = state.project;
      const shortcut = state.shortcut;
      if (!project || shortcut?.source !== 'personal') return;
      publish({ ...state, shortcutManagement: { ...state.shortcutManagement, pendingDelete: [{
        catalog: project.catalog, tool: project.tool, name: shortcut.name, path: shortcut.path,
      }] } });
    },
    cancelShortcutDeletion() {
      if (state.management.status !== 'submitting') publish({ ...state, shortcutManagement: { ...state.shortcutManagement, pendingDelete: undefined } });
    },
    async confirmShortcutDeletion() {
      const project = state.project;
      const pending = state.shortcutManagement.pendingDelete;
      if (!project || !pending?.length) return false;
      const count = pending.length;
      const names = pending.map((item) => item.name);
      const result = await mutation('delete-shortcut', 'shortcut-delete',
        count === 1 ? `Deleting shortcut ${names[0]}…` : `Deleting ${count} shortcuts…`,
        count === 1 ? `Shortcut ${names[0]} deleted.` : `${count} shortcuts deleted.`,
        { catalog: project.catalog, project: project.tool, shortcut: count === 1 ? names[0] : undefined },
        async () => {
          const deleted = await adapter.deleteShortcuts(pending);
          if (deleted !== count) throw new Error(`Expected to delete ${count} shortcuts, but the backend deleted ${deleted}.`);
          return { catalog: project.catalog, projectId: projectKey(project) };
        });
      if (result) publish({ ...state, shortcutManagement: { active: false, selected: [] } });
      else publish({ ...state, shortcutManagement: { ...state.shortcutManagement, pendingDelete: undefined } });
      return result;
    },
    async syncCatalog() {
      const catalog = state.currentCatalog;
      if (!catalog) return false;
      return mutation('sync-catalog', 'catalog-sync', `Synchronizing ${catalog}…`, `Catalog ${catalog} synchronized.`, { catalog }, async () => {
        await adapter.syncCatalog(catalog, (activity: CatalogSyncActivity) => appendActivity({
          operation: 'catalog-sync', stage: activity.stage,
          status: activity.stage === 'current' || activity.stage === 'updated' ? 'info' : 'in-progress',
          catalog: activity.catalog, detail: activity.detail,
        }));
        return { catalog };
      });
    },
    clearManagementStatus() { if (state.management.status !== 'submitting') publish({ ...state, management: { status: 'idle' } }); },
    selectBottomView(view) { publish({ ...state, bottomView: view }); },
    completeCommand(input, caret) {
      const projects = state.inventory.status === 'ready' ? state.inventory.projects : [];
      return completeLoadbotCommand(input, caret, {
        inventoryStatus: state.inventory.status,
        projects,
        currentCatalog: state.currentCatalog,
        selectedProject: state.project,
      });
    },
    submitCommand(input) {
      const submitted = input.trim();
      if (!submitted) return false;
      const projects = state.inventory.status === 'ready' ? state.inventory.projects : [];
      const result = executeLoadbotCommand(submitted, {
        inventoryStatus: state.inventory.status,
        projects,
        currentCatalog: state.currentCatalog,
        selectedProject: state.project,
      });
      publish({
        ...state,
        command: {
          entries: [...state.command.entries, { id: ++commandId, input: submitted, result }].slice(-COMMAND_HISTORY_LIMIT),
          history: [...state.command.history, submitted].slice(-COMMAND_HISTORY_LIMIT),
        },
      });
      return true;
    },
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
