import type {
  AddCatalogInput, AddProjectInput, AddShortcutInput, CatalogSyncActivityEvent, CatalogSyncStage, LoadbotAdapter, LoadbotCatalog,
  InteractiveLaunch, InteractiveSessionEvent, LoadbotProject, LoadbotShortcut,
  LoadbotRecipe, LoadbotRecipeArgument, LoadbotRunner, OperationLogActivity, ProjectOperationActivityEvent, ProjectOperationStage,
  RepositoryChange, ShortcutHelpResult, ShortcutIdentity,
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
export type ProjectFilter = 'installed' | 'all' | 'not-installed';
export type ProjectLifecycleAction = 'remove' | 'reinstall';
export type ManagementKind = 'add-catalog' | 'add-project' | 'add-shortcut' | 'update-shortcut' | 'delete-shortcut' | 'sync-catalog'
  | 'pull-project' | 'push-project' | 'update-project' | 'remove-project' | 'reinstall-project';
export type ManagementState =
  | { readonly status: 'idle' }
  | { readonly status: 'submitting'; readonly kind: ManagementKind; readonly message: string }
  | { readonly status: 'success'; readonly kind: ManagementKind; readonly message: string }
  | { readonly status: 'cancelled'; readonly kind: ManagementKind; readonly message: string }
  | { readonly status: 'error'; readonly kind: ManagementKind; readonly message: string };
export type ActivityOperation = 'catalog-sync' | 'catalog-add' | 'project-add' | 'shortcut-add' | 'shortcut-update' | 'shortcut-delete' | 'local-reload'
  | 'project-folder-open' | 'project-pull' | 'project-push' | 'project-update' | 'project-remove' | 'project-reinstall';
export type ActivityStatus = 'in-progress' | 'info' | 'success' | 'error' | 'cancelled';
export type ActivityStage = CatalogSyncStage | ProjectOperationStage | 'started' | 'interactive-authentication'
  | 'authoritative-reload' | 'catalog-state' | 'completed' | 'failed' | 'cancelled';
export interface ActivityEntry {
  readonly id: number;
  readonly operationId: string;
  readonly timestamp: string;
  readonly operation: ActivityOperation;
  readonly stage: ActivityStage;
  readonly status: ActivityStatus;
  readonly catalog?: string;
  readonly project?: string;
  readonly shortcut?: string;
  readonly detail?: string;
}
export interface ActivityLogEntry {
  readonly id: number;
  readonly operationId: string;
  readonly timestamp: string;
  readonly stream: OperationLogActivity['stream'];
  readonly text: string;
}
export const ACTIVITY_HISTORY_LIMIT = 250;
export const ACTIVITY_LOG_HISTORY_LIMIT = 1000;
export const COMMAND_HISTORY_LIMIT = 100;
export const INTERACTIVE_TRANSCRIPT_LIMIT = 500;
export const TERMINAL_TRANSCRIPT_LIMIT = 256 * 1024;
export interface CommandEntry {
  readonly id: number;
  readonly input: string;
  readonly result: CommandResult;
}
export interface CommandState {
  readonly entries: readonly CommandEntry[];
  readonly history: readonly string[];
  readonly interactive?: {
    readonly status: 'starting' | 'active' | 'terminating';
    readonly launchId: string;
    readonly label: string;
    readonly sessionId?: string;
    readonly processId?: string;
  };
  readonly interactiveTranscript: readonly {
    readonly id: number;
    readonly kind: 'output' | 'system' | 'error';
    readonly text: string;
  }[];
}
export interface ProjectTerminalState {
  readonly status: 'idle' | 'starting' | 'active' | 'terminating' | 'exited' | 'error';
  readonly project?: { readonly catalog: string; readonly tool: string };
  readonly transcript: string;
  readonly launchId?: string;
  readonly sessionId?: string;
  readonly processId?: string;
  readonly exitCode?: number;
  readonly signal?: string;
  readonly cancelled?: boolean;
  readonly message?: string;
}
export type ShortcutHelpState =
  | { readonly status: 'loading' }
  | { readonly status: 'ready'; readonly result: ShortcutHelpResult }
  | { readonly status: 'error'; readonly message: string };

export interface LoadbotState {
  readonly inventory: InventoryState;
  readonly catalogState: CatalogState;
  readonly currentCatalog?: string;
  readonly projectFilter: ProjectFilter;
  readonly project?: LoadbotProject;
  readonly shortcut?: LoadbotShortcut;
  readonly fields: readonly SampleField[];
  readonly values: SampleValues;
  readonly missingInputIds: readonly string[];
  readonly drawerOpen: boolean;
  readonly bottomView: 'command' | 'activity' | 'terminal';
  readonly command: CommandState;
  readonly activity: readonly ActivityEntry[];
  readonly activityLogs: readonly ActivityLogEntry[];
  readonly management: ManagementState;
  readonly recipeEditor?: { readonly draft: RecipeDraft; readonly errors: readonly string[]; readonly help?: ShortcutHelpState };
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
  readonly projectTerminal: ProjectTerminalState;
  readonly pendingProjectAction?: { readonly action: ProjectLifecycleAction; readonly project: LoadbotProject };
  readonly pendingCommitPush?: {
    readonly project: LoadbotProject;
    readonly changedFiles: readonly RepositoryChange[];
    readonly selectedPaths: readonly string[];
    readonly commitMessage: string;
    readonly operationId: string;
  };
}

export interface LoadbotActions {
  selectCatalog(name: string): void;
  selectProject(id: string): void;
  selectProjectFilter(filter: ProjectFilter): void;
  selectShortcut(id: string): void;
  reloadInventory(): void;
  openProjectFolder(id: string): void;
  openProjectTerminal(id: string): void;
  pullProject(project?: LoadbotProject): Promise<boolean>;
  pushProject(project?: LoadbotProject): Promise<boolean>;
  toggleCommitPushPath(path: string): void;
  setCommitPushMessage(message: string): void;
  cancelCommitPush(): void;
  confirmCommitPush(): Promise<boolean>;
  updateProject(project?: LoadbotProject): Promise<boolean>;
  removeProject(project?: LoadbotProject): Promise<boolean>;
  reinstallProject(project?: LoadbotProject): Promise<boolean>;
  requestProjectAction(action: ProjectLifecycleAction, project?: LoadbotProject): void;
  cancelProjectAction(): void;
  confirmProjectAction(): Promise<boolean>;
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
  viewRecipeHelp(): Promise<boolean>;
  dismissRecipeHelp(): void;
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
  selectBottomView(view: 'command' | 'activity' | 'terminal'): void;
  completeCommand(input: string, caret: number): CommandCompletion | undefined;
  submitCommand(input: string): boolean;
  startInteractiveSession(launch: InteractiveLaunch): Promise<boolean>;
  cancelInteractiveSession(): Promise<boolean>;
  sendProjectTerminalInput(input: string): boolean;
  resizeProjectTerminal(columns: number, rows: number): boolean;
  closeProjectTerminal(): Promise<boolean>;
  restartProjectTerminal(): Promise<boolean>;
  changeSampleInput(id: string, value: string | boolean): void;
  useSamplePath(id: string): void;
  toggleDrawer(): void;
}

interface ReadPreference { catalog?: string; projectId?: string; shortcutId?: string; shortcutName?: string; projectFilter?: ProjectFilter }

/** Deterministic application state, independent of React, DOM, themes and hosts.
 * Backend-confirmed writes are always followed by an authoritative workspace read.
 */
export function createLoadbotApplication(adapter: LoadbotAdapter, sampleForms: SampleForms = noSampleForms) {
  let state: LoadbotState = {
    inventory: { status: 'loading' }, catalogState: { status: 'loading' }, projectFilter: 'installed', fields: [], values: {},
    missingInputIds: [], drawerOpen: true, bottomView: 'command',
    command: { entries: [], history: [], interactiveTranscript: [] },
    activity: [], activityLogs: [], management: { status: 'idle' }, shortcutManagement: { active: false, selected: [] },
    projectFolder: { status: 'idle' }, projectTerminal: { status: 'idle', transcript: '' },
  };
  const listeners = new Set<() => void>();
  let generation = 0;
  let folderGeneration = 0;
  let terminalGeneration = 0;
  let terminalInputChain = Promise.resolve();
  let activityId = 0;
  let operationId = 0;
  let activityLogId = 0;
  let commandId = 0;
  let interactiveTranscriptId = 0;
  let interactiveGeneration = 0;
  const pendingInteractiveLaunches: InteractiveLaunch[] = [];
  let recipeArgumentKey = 1000;
  let recipeHelpGeneration = 0;
  const publish = (next: LoadbotState) => {
    state = next;
    listeners.forEach((listener) => listener());
  };
  const selection = (project?: LoadbotProject, shortcut = project?.entries[0]) => {
    const fields = project && shortcut ? sampleForms[selectionKey(project, shortcut)] ?? [] : [];
    const values = initialValues(fields);
    return { project, shortcut, fields, values, missingInputIds: missingInputs(fields, values) };
  };
  const projectsFor = (projects: readonly LoadbotProject[], catalog?: string, filter: ProjectFilter = state.projectFilter) =>
    projects.filter((project) => (!catalog || project.catalog === catalog)
      && (filter === 'all' || (filter === 'installed' ? project.installed !== false : project.installed === false)));
  const appendActivity = (entry: Omit<ActivityEntry, 'id' | 'timestamp'>, reveal = true) => {
    const activity = [...state.activity, { ...entry, id: ++activityId, timestamp: new Date().toISOString() }]
      .slice(-ACTIVITY_HISTORY_LIMIT);
    publish({ ...state, activity, bottomView: reveal ? 'activity' : state.bottomView });
  };
  const beginActivity = (entry: Omit<ActivityEntry, 'id' | 'timestamp' | 'operationId'>) => {
    const id = `operation-${++operationId}`;
    appendActivity({ ...entry, operationId: id });
    return id;
  };
  const appendLog = (operation: string, log: OperationLogActivity) => {
    const activityLogs = [...state.activityLogs, {
      id: ++activityLogId, operationId: operation, timestamp: new Date().toISOString(), stream: log.stream, text: log.text,
    }].slice(-ACTIVITY_LOG_HISTORY_LIMIT);
    publish({ ...state, activityLogs, bottomView: 'activity' });
  };
  const errorMessage = (error: unknown, fallback: string) => error instanceof Error ? error.message : fallback;
  const appendInteractiveTranscript = (kind: 'output' | 'system' | 'error', text: string) => {
    if (!text) return;
    const interactiveTranscript = [...state.command.interactiveTranscript, {
      id: ++interactiveTranscriptId, kind, text,
    }].slice(-INTERACTIVE_TRANSCRIPT_LIMIT);
    publish({ ...state, bottomView: 'command', command: { ...state.command, interactiveTranscript } });
  };
  const cancelled = (error: unknown) => Boolean(error && typeof error === 'object' && 'kind' in error && error.kind === 'cancelled');
  const projectProgress = (id: string, operation: ActivityOperation) => (activity: ProjectOperationActivityEvent) => {
    if ('kind' in activity && activity.kind === 'log') appendLog(id, activity);
    else if ('kind' in activity && activity.kind === 'interactive-launch') {
      appendActivity({
        operationId: id, operation, stage: 'interactive-authentication', status: 'in-progress',
        detail: 'Interactive authentication required.',
      });
      if (state.command.interactive) pendingInteractiveLaunches.push(activity);
      else void actions.startInteractiveSession(activity);
    }
    else appendActivity({
      operationId: id, operation, stage: activity.stage, status: 'in-progress', catalog: activity.catalog, project: activity.tool,
    });
  };

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
    const projectFilter = preferred.projectFilter ?? state.projectFilter;
    folderGeneration++;
    publish({ ...state, inventory: { status: 'loading' }, catalogState: { status: 'loading' }, projectFilter, ...selection(), projectFolder: { status: 'idle' } });
    try {
      const [projects, catalogs] = await Promise.all([adapter.readInventory(), adapter.readCatalogs()]);
      if (request !== generation) return false;
      const catalogNames = new Set(catalogs.map((catalog) => catalog.name));
      const currentCatalog = selectedCatalog && (catalogNames.has(selectedCatalog) || projects.some((project) => project.catalog === selectedCatalog))
        ? selectedCatalog
        : catalogs.find((catalog) => catalog.default)?.name ?? projects[0]?.catalog ?? catalogs[0]?.name;
      const visible = projectsFor(projects, currentCatalog, projectFilter);
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
        projectFilter, ...selection(project, shortcut), shortcutManagement,
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
    operation: (operationId: string) => Promise<ReadPreference>,
  ): Promise<boolean> {
    if (state.management.status === 'submitting') return false;
    publish({ ...state, management: { status: 'submitting', kind, message: progress } });
    const id = beginActivity({ operation: activityOperation, stage: 'started', status: 'in-progress', ...context });
    try {
      const preferred = await operation(id);
      appendActivity({ operationId: id, operation: activityOperation, stage: 'authoritative-reload', status: 'in-progress', ...context });
      const reloaded = await beginWorkspaceRead(preferred);
      if (reloaded && activityOperation === 'catalog-sync' && state.catalogState.status === 'ready') {
        const catalog = state.catalogState.catalogs.find((item) => item.name === preferred.catalog);
        if (catalog) appendActivity({
          operationId: id,
          operation: activityOperation, stage: 'catalog-state', status: 'info', catalog: catalog.name,
          detail: `${catalog.state} · ${catalog.writable ? 'writable' : 'read-only'}`,
        });
      }
      publish({
        ...state,
        management: reloaded
          ? { status: 'success', kind, message: completed }
          : { status: 'error', kind, message: `${completed} Local state could not be reread; use Reload.` },
      });
      appendActivity({
        operationId: id,
        operation: activityOperation, stage: reloaded ? 'completed' : 'failed', status: reloaded ? 'success' : 'error',
        ...context, detail: reloaded ? completed : 'The operation completed, but local state could not be reread.',
      });
      return true;
    } catch (error: unknown) {
      const message = errorMessage(error, 'The Loadbot operation failed.');
      const wasCancelled = cancelled(error);
      // Some shared operations can report a failure after an earlier durable step
      // (for example, a catalog definition saved before an explicit push fails).
      // Never guess: reread local authority before presenting the failure.
      appendActivity({ operationId: id, operation: activityOperation, stage: 'authoritative-reload', status: 'in-progress', ...context });
      await beginWorkspaceRead();
      publish({
        ...state,
        management: { status: wasCancelled ? 'cancelled' : 'error', kind, message },
      });
      appendActivity({
        operationId: id, operation: activityOperation, stage: wasCancelled ? 'cancelled' : 'failed',
        status: wasCancelled ? 'cancelled' : 'error', ...context, detail: message,
      });
      return false;
    }
  }

  async function completeProjectPush(
    id: string,
    target: LoadbotProject,
    operation: () => Promise<{ catalog: string; tool: string }>,
  ): Promise<boolean> {
    const context = { catalog: target.catalog, project: target.tool };
    try {
      const pushed = await operation();
      appendActivity({ operationId: id, operation: 'project-push', stage: 'authoritative-reload', status: 'in-progress', ...context });
      const reloaded = await beginWorkspaceRead({ catalog: pushed.catalog, projectId: projectKey(pushed) });
      const message = reloaded
        ? `Project ${target.tool} pushed.`
        : `Project ${target.tool} pushed. Local state could not be reread; use Reload.`;
      publish({
        ...state,
        pendingCommitPush: undefined,
        management: reloaded
          ? { status: 'success', kind: 'push-project', message }
          : { status: 'error', kind: 'push-project', message },
      });
      appendActivity({
        operationId: id, operation: 'project-push', stage: reloaded ? 'completed' : 'failed',
        status: reloaded ? 'success' : 'error', ...context, detail: message,
      });
      return reloaded;
    } catch (error: unknown) {
      const message = errorMessage(error, 'The Push operation failed.');
      const wasCancelled = cancelled(error);
      appendActivity({ operationId: id, operation: 'project-push', stage: 'authoritative-reload', status: 'in-progress', ...context });
      await beginWorkspaceRead();
      publish({
        ...state,
        pendingCommitPush: undefined,
        management: { status: wasCancelled ? 'cancelled' : 'error', kind: 'push-project', message },
      });
      appendActivity({
        operationId: id, operation: 'project-push', stage: wasCancelled ? 'cancelled' : 'failed',
        status: wasCancelled ? 'cancelled' : 'error', ...context, detail: message,
      });
      return false;
    }
  }

  const sameProject = (left: { catalog: string; tool: string } | undefined, right: { catalog: string; tool: string }) =>
    left?.catalog === right.catalog && left.tool === right.tool;
  const boundedTerminalTranscript = (current: string, text: string) =>
    `${current}${text}`.slice(-TERMINAL_TRANSCRIPT_LIMIT);

  async function startProjectTerminal(
    project: { catalog: string; tool: string },
    restart = false,
  ): Promise<boolean> {
    const current = state.projectTerminal;
    if (!restart && current.project) {
      return sameProject(current.project, project) && current.status === 'active';
    }
    if (!adapter.createProjectTerminalLaunch || !adapter.startInteractiveSession
      || !adapter.sendInteractiveInput || !adapter.terminateInteractiveSession) {
      publish({
        ...state,
        projectTerminal: {
          status: 'error', project, transcript: '',
          message: 'Embedded project terminals are unavailable in this host.',
        },
      });
      return false;
    }
    const generation = ++terminalGeneration;
    publish({ ...state, projectTerminal: { status: 'starting', project, transcript: '' } });
    try {
      const launch = await adapter.createProjectTerminalLaunch(project);
      if (generation !== terminalGeneration) return false;
      publish({
        ...state,
        projectTerminal: { ...state.projectTerminal, launchId: launch.launchId },
      });
      const onEvent = (event: InteractiveSessionEvent) => {
        if (generation !== terminalGeneration) return;
        const terminal = state.projectTerminal;
        if (!sameProject(terminal.project, project)) return;
        if (event.kind === 'output') {
          publish({
            ...state,
            projectTerminal: {
              ...terminal,
              transcript: boundedTerminalTranscript(terminal.transcript, event.text),
            },
          });
          return;
        }
        if (event.kind === 'failed') {
          publish({
            ...state,
            projectTerminal: {
              ...terminal, status: 'error', message: event.message,
            },
          });
          return;
        }
        publish({
          ...state,
          projectTerminal: {
            ...terminal, status: 'exited', exitCode: event.code, signal: event.signal,
            cancelled: event.cancelled, sessionId: undefined,
          },
        });
      };
      const started = await adapter.startInteractiveSession(launch, onEvent);
      if (generation !== terminalGeneration) return false;
      if (state.projectTerminal.status !== 'starting') return true;
      publish({
        ...state,
        projectTerminal: {
          ...state.projectTerminal, status: 'active', sessionId: started.sessionId,
          processId: started.processId,
        },
      });
      return true;
    } catch (error: unknown) {
      if (generation === terminalGeneration) {
        publish({
          ...state,
          projectTerminal: {
            ...state.projectTerminal, status: 'error', project,
            message: errorMessage(error, 'Could not start the project terminal.'),
          },
        });
      }
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
    selectProjectFilter(filter) {
      if (filter === state.projectFilter) return;
      const projects = state.inventory.status === 'ready' ? projectsFor(state.inventory.projects, state.currentCatalog, filter) : [];
      const selectedId = state.project && projectKey(state.project);
      const current = selectedId ? projects.find((project) => projectKey(project) === selectedId) : undefined;
      folderGeneration++;
      publish({ ...state, projectFilter: filter, ...selection(current ?? projects[0]), projectFolder: { status: 'idle' }, shortcutManagement: { active: false, selected: [] } });
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
      const id = beginActivity({ operation: 'local-reload', stage: 'started', status: 'in-progress' });
      void beginWorkspaceRead().then((reloaded) => appendActivity({
        operationId: id,
        operation: 'local-reload', stage: reloaded ? 'completed' : 'failed', status: reloaded ? 'success' : 'error',
        detail: reloaded ? 'Local inventory and catalog context reread.' : 'Could not reread local Loadbot state.',
      }));
    },
    openProjectFolder(id) {
      if (state.inventory.status !== 'ready') return;
      const project = projectsFor(state.inventory.projects, state.currentCatalog).find((item) => projectKey(item) === id);
      if (!project || project.installed === false) return;
      const request = ++folderGeneration;
      publish({ ...state, projectFolder: { status: 'opening', projectId: id } });
      const identity = { catalog: project.catalog, tool: project.tool };
      const operation = beginActivity({ operation: 'project-folder-open', stage: 'started', status: 'in-progress', catalog: project.catalog, project: project.tool });
      Promise.resolve().then(() => adapter.openProjectFolder(identity)).then(
        () => {
          if (request === folderGeneration) {
            publish({ ...state, projectFolder: { status: 'opened', projectId: id, message: `Opened ${project.tool}.` } });
          }
          appendActivity({ operationId: operation, operation: 'project-folder-open', stage: 'completed', status: 'success', catalog: project.catalog, project: project.tool });
        },
        (error: unknown) => {
          const message = errorMessage(error, 'Could not open the project folder.');
          if (request === folderGeneration) {
            publish({ ...state, projectFolder: { status: 'error', projectId: id, message } });
          }
          appendActivity({ operationId: operation, operation: 'project-folder-open', stage: 'failed', status: 'error', catalog: project.catalog, project: project.tool, detail: message });
        },
      );
    },
    openProjectTerminal(id) {
      if (state.inventory.status !== 'ready') return;
      const project = projectsFor(state.inventory.projects, state.currentCatalog).find((item) => projectKey(item) === id);
      if (!project) return;
      publish({ ...state, bottomView: 'terminal', drawerOpen: true });
      if (project.installed !== false && !state.projectTerminal.project) {
        void startProjectTerminal({ catalog: project.catalog, tool: project.tool });
      }
    },
    async pullProject(project?: LoadbotProject) {
      const target = project ?? state.project;
      if (!target || target.installed !== false || !adapter.pullProject) return false;
      return mutation('pull-project', 'project-pull', `Pulling ${target.tool}…`, `Project ${target.tool} pulled.`, { catalog: target.catalog, project: target.tool }, async (operation) => {
        const installed = await adapter.pullProject!({ catalog: target.catalog, tool: target.tool }, projectProgress(operation, 'project-pull'));
        return { catalog: installed.catalog, projectId: projectKey(installed), projectFilter: 'installed' };
      });
    },
    async updateProject(project?: LoadbotProject) {
      const target = project ?? state.project;
      if (!target || target.installed === false || !adapter.updateProject) return false;
      return mutation('update-project', 'project-update', `Updating ${target.tool}…`, `Project ${target.tool} updated.`, { catalog: target.catalog, project: target.tool }, async (operation) => {
        const updated = await adapter.updateProject!({ catalog: target.catalog, tool: target.tool }, projectProgress(operation, 'project-update'));
        return { catalog: updated.catalog, projectId: projectKey(updated) };
      });
    },
    async pushProject(project?: LoadbotProject) {
      const target = project ?? state.project;
      if (!target || target.installed === false || !adapter.pushProject || state.command.interactive) return false;
      if (!adapter.inspectProjectPush) {
        return mutation('push-project', 'project-push', `Pushing ${target.tool}…`, `Project ${target.tool} pushed.`, { catalog: target.catalog, project: target.tool }, async (operation) => {
          const pushed = await adapter.pushProject!({ catalog: target.catalog, tool: target.tool }, projectProgress(operation, 'project-push'));
          return { catalog: pushed.catalog, projectId: projectKey(pushed) };
        });
      }
      if (state.management.status === 'submitting' || state.pendingCommitPush) return false;
      publish({ ...state, management: { status: 'submitting', kind: 'push-project', message: `Inspecting ${target.tool}…` } });
      const id = beginActivity({ operation: 'project-push', stage: 'started', status: 'in-progress', catalog: target.catalog, project: target.tool });
      try {
        const inspection = await adapter.inspectProjectPush(
          { catalog: target.catalog, tool: target.tool },
          projectProgress(id, 'project-push'),
        );
        if (inspection.changedFiles.length) {
          publish({
            ...state,
            management: { status: 'success', kind: 'push-project', message: 'Review changed files before committing.' },
            pendingCommitPush: {
              project: target,
              changedFiles: inspection.changedFiles,
              selectedPaths: inspection.changedFiles.map((change) => change.path),
              commitMessage: '',
              operationId: id,
            },
          });
          return true;
        }
        if (!inspection.commitsAhead) {
          const current = 'Nothing to push. Project is already current.';
          appendActivity({ operationId: id, operation: 'project-push', stage: 'authoritative-reload', status: 'in-progress', catalog: target.catalog, project: target.tool });
          const reloaded = await beginWorkspaceRead({ catalog: target.catalog, projectId: projectKey(target) });
          const message = reloaded ? current : `${current} Local state could not be reread; use Reload.`;
          publish({ ...state, management: { status: reloaded ? 'success' : 'error', kind: 'push-project', message } });
          appendActivity({
            operationId: id, operation: 'project-push', stage: reloaded ? 'completed' : 'failed',
            status: reloaded ? 'success' : 'error', catalog: target.catalog, project: target.tool, detail: message,
          });
          return reloaded;
        }
        publish({ ...state, management: { status: 'submitting', kind: 'push-project', message: `Pushing ${target.tool}…` } });
        return completeProjectPush(id, target, () => adapter.pushProject!(
          { catalog: target.catalog, tool: target.tool }, projectProgress(id, 'project-push'),
        ));
      } catch (error: unknown) {
        const message = errorMessage(error, 'Could not inspect the project for Push.');
        publish({ ...state, management: { status: 'error', kind: 'push-project', message } });
        appendActivity({ operationId: id, operation: 'project-push', stage: 'failed', status: 'error', catalog: target.catalog, project: target.tool, detail: message });
        return false;
      }
    },
    toggleCommitPushPath(path) {
      const pending = state.pendingCommitPush;
      if (!pending || state.management.status === 'submitting' || !pending.changedFiles.some((change) => change.path === path)) return;
      const selectedPaths = pending.selectedPaths.includes(path)
        ? pending.selectedPaths.filter((selected) => selected !== path)
        : [...pending.selectedPaths, path];
      publish({ ...state, pendingCommitPush: { ...pending, selectedPaths } });
    },
    setCommitPushMessage(message) {
      const pending = state.pendingCommitPush;
      if (!pending || state.management.status === 'submitting') return;
      publish({ ...state, pendingCommitPush: { ...pending, commitMessage: message } });
    },
    cancelCommitPush() {
      const pending = state.pendingCommitPush;
      if (!pending || state.management.status === 'submitting') return;
      publish({ ...state, pendingCommitPush: undefined, management: { status: 'cancelled', kind: 'push-project', message: 'Commit & Push cancelled; no Git changes were made.' } });
      appendActivity({
        operationId: pending.operationId, operation: 'project-push', stage: 'cancelled', status: 'cancelled',
        catalog: pending.project.catalog, project: pending.project.tool,
        detail: 'Commit & Push cancelled before confirmation.',
      });
    },
    async confirmCommitPush() {
      const pending = state.pendingCommitPush;
      if (!pending || state.management.status === 'submitting' || !adapter.commitAndPushProject
        || !pending.selectedPaths.length || !pending.commitMessage.trim()) return false;
      publish({
        ...state,
        pendingCommitPush: undefined,
        management: { status: 'submitting', kind: 'push-project', message: `Committing and pushing ${pending.project.tool}…` },
      });
      return completeProjectPush(pending.operationId, pending.project, () => adapter.commitAndPushProject!({
        catalog: pending.project.catalog,
        tool: pending.project.tool,
        selectedPaths: pending.selectedPaths,
        commitMessage: pending.commitMessage,
      }, projectProgress(pending.operationId, 'project-push')));
    },
    async removeProject(project?: LoadbotProject) {
      const target = project ?? state.project;
      if (!target || target.installed === false || !adapter.removeProject) return false;
      return mutation('remove-project', 'project-remove', `Removing ${target.tool}…`, `Project ${target.tool} removed.`, { catalog: target.catalog, project: target.tool }, async (operation) => {
        const removed = await adapter.removeProject!({ catalog: target.catalog, tool: target.tool }, projectProgress(operation, 'project-remove'));
        return { catalog: removed.catalog, projectId: projectKey(removed), projectFilter: 'not-installed' };
      });
    },
    async reinstallProject(project?: LoadbotProject) {
      const target = project ?? state.project;
      if (!target || target.installed === false || !adapter.reinstallProject) return false;
      return mutation('reinstall-project', 'project-reinstall', `Reinstalling ${target.tool}…`, `Project ${target.tool} reinstalled.`, { catalog: target.catalog, project: target.tool }, async (operation) => {
        const reinstalled = await adapter.reinstallProject!({ catalog: target.catalog, tool: target.tool }, projectProgress(operation, 'project-reinstall'));
        return { catalog: reinstalled.catalog, projectId: projectKey(reinstalled), projectFilter: 'installed' };
      });
    },
    requestProjectAction(action, project?: LoadbotProject) {
      const target = project ?? state.project;
      if (!target || target.installed === false || state.management.status === 'submitting') return;
      publish({ ...state, pendingProjectAction: { action, project: target } });
    },
    cancelProjectAction() {
      if (state.management.status !== 'submitting') publish({ ...state, pendingProjectAction: undefined });
    },
    async confirmProjectAction() {
      const pending = state.pendingProjectAction;
      if (!pending) return false;
      const { project, action } = pending;
      const capability = action === 'remove' ? adapter.removeProject : adapter.reinstallProject;
      if (!capability) return false;
      const result = await mutation(
        action === 'remove' ? 'remove-project' : 'reinstall-project',
        action === 'remove' ? 'project-remove' : 'project-reinstall',
        `${action === 'remove' ? 'Removing' : 'Reinstalling'} ${project.tool}…`,
        `Project ${project.tool} ${action === 'remove' ? 'removed' : 'reinstalled'}.`,
        { catalog: project.catalog, project: project.tool },
        async (operation) => {
          const changed = await capability.call(adapter, { catalog: project.catalog, tool: project.tool },
            projectProgress(operation, action === 'remove' ? 'project-remove' : 'project-reinstall'));
          return { catalog: changed.catalog, projectId: projectKey(changed), projectFilter: action === 'remove' ? 'not-installed' : 'installed' };
        },
      );
      if (result) publish({ ...state, pendingProjectAction: undefined });
      return result;
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
      recipeHelpGeneration++;
      publish({ ...state, recipeEditor: { draft: newRecipeDraft(), errors: [] } });
    },
    openSelectedRecipeEditor() {
      const draft = state.shortcut && recipeDraftFromShortcut(state.shortcut);
      if (!draft) return false;
      recipeHelpGeneration++;
      publish({ ...state, recipeEditor: { draft, errors: [] }, management: { status: 'idle' } });
      return true;
    },
    closeRecipeEditor() {
      if (state.management.status !== 'submitting') {
        recipeHelpGeneration++;
        publish({ ...state, recipeEditor: undefined });
      }
    },
    updateRecipeDetails(values) {
      if (!state.recipeEditor) return;
      publish({ ...state, recipeEditor: { ...state.recipeEditor, draft: { ...state.recipeEditor.draft, ...values }, errors: [] } });
    },
    setRecipeTarget(target) {
      if (!state.recipeEditor) return;
      recipeHelpGeneration++;
      publish({ ...state, recipeEditor: { draft: updateDraftTarget(state.recipeEditor.draft, target), errors: [] } });
    },
    setRecipeRunner(runner) {
      if (!state.recipeEditor) return;
      recipeHelpGeneration++;
      publish({ ...state, recipeEditor: { draft: updateDraftRunner(state.recipeEditor.draft, runner), errors: [] } });
    },
    setRecipeWorkingDirectory(working_directory) {
      if (!state.recipeEditor) return;
      recipeHelpGeneration++;
      publish({ ...state, recipeEditor: { draft: { ...state.recipeEditor.draft, workingDirectory: working_directory }, errors: [] } });
    },
    addRecipeParameter(kind) {
      if (!state.recipeEditor) return;
      publish({ ...state, recipeEditor: { ...state.recipeEditor, draft: addDraftArgument(state.recipeEditor.draft, kind, ++recipeArgumentKey), errors: [] } });
    },
    updateRecipeParameter(key, value, idManuallyEdited) {
      if (!state.recipeEditor) return;
      publish({ ...state, recipeEditor: { ...state.recipeEditor, draft: updateDraftArgument(state.recipeEditor.draft, key, value, idManuallyEdited), errors: [] } });
    },
    removeRecipeParameter(key) {
      if (!state.recipeEditor) return;
      publish({ ...state, recipeEditor: { ...state.recipeEditor, draft: removeDraftArgument(state.recipeEditor.draft, key), errors: [] } });
    },
    moveRecipeParameter(key, direction) {
      if (!state.recipeEditor) return;
      publish({ ...state, recipeEditor: { ...state.recipeEditor, draft: moveDraftArgument(state.recipeEditor.draft, key, direction), errors: [] } });
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
    async viewRecipeHelp() {
      const editor = state.recipeEditor;
      const project = state.project;
      if (!editor || !project || editor.help?.status === 'loading') return false;
      if (!editor.draft.target.trim()) {
        publish({ ...state, recipeEditor: { ...editor, help: { status: 'error', message: 'Choose a target before viewing help.' } } });
        return false;
      }
      const request = ++recipeHelpGeneration;
      publish({ ...state, recipeEditor: { ...editor, help: { status: 'loading' } } });
      try {
        const result = await adapter.viewShortcutHelp({
          catalog: project.catalog, tool: project.tool, target: editor.draft.target.trim(),
          runner: editor.draft.runner, workingDirectory: editor.draft.workingDirectory,
        });
        if (request !== recipeHelpGeneration || !state.recipeEditor) return false;
        publish({ ...state, recipeEditor: { ...state.recipeEditor, help: { status: 'ready', result } } });
        return true;
      } catch (error: unknown) {
        if (request !== recipeHelpGeneration || !state.recipeEditor) return false;
        publish({ ...state, recipeEditor: { ...state.recipeEditor, help: {
          status: 'error', message: errorMessage(error, 'Could not view help for this target.'),
        } } });
        return false;
      }
    },
    dismissRecipeHelp() {
      if (!state.recipeEditor) return;
      recipeHelpGeneration++;
      publish({ ...state, recipeEditor: { ...state.recipeEditor, help: undefined } });
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
      return mutation('sync-catalog', 'catalog-sync', `Refreshing ${catalog}…`, `Catalog ${catalog} refreshed.`, { catalog }, async (operation) => {
        await adapter.syncCatalog(catalog, (activity: CatalogSyncActivityEvent) => {
          if ('kind' in activity) appendLog(operation, activity);
          else appendActivity({
            operationId: operation, operation: 'catalog-sync', stage: activity.stage,
            status: activity.stage === 'current' || activity.stage === 'updated' ? 'info' : 'in-progress',
            catalog: activity.catalog, detail: activity.detail,
          });
        });
        return { catalog };
      });
    },
    clearManagementStatus() { if (state.management.status !== 'submitting') publish({ ...state, management: { status: 'idle' } }); },
    selectBottomView(view) {
      publish({ ...state, bottomView: view });
      if (view === 'terminal' && state.project?.installed !== false && !state.projectTerminal.project && state.project) {
        void startProjectTerminal({ catalog: state.project.catalog, tool: state.project.tool });
      }
    },
    completeCommand(input, caret) {
      if (state.command.interactive) return undefined;
      const projects = state.inventory.status === 'ready' ? state.inventory.projects : [];
      return completeLoadbotCommand(input, caret, {
        inventoryStatus: state.inventory.status,
        projects,
        currentCatalog: state.currentCatalog,
        selectedProject: state.project,
      });
    },
    submitCommand(input) {
      const interactive = state.command.interactive;
      if (interactive) {
        if (interactive.status !== 'active' || !interactive.sessionId || !adapter.sendInteractiveInput) return false;
        const generation = interactiveGeneration;
        void adapter.sendInteractiveInput(interactive.sessionId, `${input}\r`).catch((error: unknown) => {
          if (generation === interactiveGeneration) {
            appendInteractiveTranscript('error', `${errorMessage(error, 'Could not send interactive input.')}\n`);
          }
        });
        return true;
      }
      const submitted = input.trim();
      if (!submitted) return false;
      const projects = state.inventory.status === 'ready' ? state.inventory.projects : [];
      const result = executeLoadbotCommand(submitted, {
        inventoryStatus: state.inventory.status,
        projects,
        currentCatalog: state.currentCatalog,
        selectedProject: state.project,
      });
      if (result.kind === 'lifecycle') {
        switch (result.action) {
          case 'pull':
            void actions.pullProject(result.project);
            break;
          case 'push':
            void actions.pushProject(result.project);
            break;
          case 'update':
            void actions.updateProject(result.project);
            break;
          case 'remove':
          case 'reinstall':
            actions.requestProjectAction(result.action, result.project);
            break;
        }
      }
      publish({
        ...state,
        command: {
          ...state.command,
          entries: [...state.command.entries, { id: ++commandId, input: submitted, result }].slice(-COMMAND_HISTORY_LIMIT),
          history: [...state.command.history, submitted].slice(-COMMAND_HISTORY_LIMIT),
        },
      });
      return true;
    },
    async startInteractiveSession(launch) {
      if (state.command.interactive || !adapter.startInteractiveSession
        || !adapter.sendInteractiveInput || !adapter.terminateInteractiveSession) return false;
      const generation = ++interactiveGeneration;
      publish({
        ...state, bottomView: 'command',
        command: {
          ...state.command,
          interactive: { status: 'starting', launchId: launch.launchId, label: launch.label },
        },
      });
      appendInteractiveTranscript('system', `Interactive session starting: ${launch.label}\n`);
      const onEvent = (event: InteractiveSessionEvent) => {
        if (generation !== interactiveGeneration) return;
        if (event.kind === 'output') {
          appendInteractiveTranscript('output', event.text);
          return;
        }
        const text = event.kind === 'failed'
          ? `Interactive session failed: ${event.message}\n`
          : event.cancelled
            ? 'Interactive session cancelled.\n'
            : `Interactive session exited with code ${event.code}${event.signal ? ` (${event.signal})` : ''}.\n`;
        const entry = {
          id: ++interactiveTranscriptId,
          kind: event.kind === 'failed' ? 'error' as const : 'system' as const,
          text,
        };
        publish({
          ...state, bottomView: 'command',
          command: {
            ...state.command, interactive: undefined,
            interactiveTranscript: [...state.command.interactiveTranscript, entry].slice(-INTERACTIVE_TRANSCRIPT_LIMIT),
          },
        });
        const next = pendingInteractiveLaunches.shift();
        if (next) void actions.startInteractiveSession(next);
      };
      try {
        const started = await adapter.startInteractiveSession(launch, onEvent);
        if (generation !== interactiveGeneration || !state.command.interactive) return true;
        publish({
          ...state,
          command: {
            ...state.command,
            interactive: {
              status: 'active', launchId: launch.launchId, label: launch.label,
              sessionId: started.sessionId, processId: started.processId,
            },
          },
        });
        return true;
      } catch (error: unknown) {
        if (generation === interactiveGeneration) {
          const entry = {
            id: ++interactiveTranscriptId, kind: 'error' as const,
            text: `${errorMessage(error, 'Could not start the interactive session.')}\n`,
          };
          publish({
            ...state,
            command: {
              ...state.command, interactive: undefined,
              interactiveTranscript: [...state.command.interactiveTranscript, entry].slice(-INTERACTIVE_TRANSCRIPT_LIMIT),
            },
          });
        }
        return false;
      }
    },
    async cancelInteractiveSession() {
      const interactive = state.command.interactive;
      if (!interactive?.sessionId || interactive.status !== 'active' || !adapter.terminateInteractiveSession) return false;
      const generation = interactiveGeneration;
      publish({
        ...state,
        command: { ...state.command, interactive: { ...interactive, status: 'terminating' } },
      });
      try {
        await adapter.terminateInteractiveSession(interactive.sessionId);
        return true;
      } catch (error: unknown) {
        if (generation === interactiveGeneration && state.command.interactive?.sessionId === interactive.sessionId) {
          publish({
            ...state,
            command: { ...state.command, interactive: { ...interactive, status: 'active' } },
          });
          appendInteractiveTranscript('error', `${errorMessage(error, 'Could not terminate the interactive session.')}\n`);
        }
        return false;
      }
    },
    sendProjectTerminalInput(input) {
      const terminal = state.projectTerminal;
      if (terminal.status !== 'active' || !terminal.sessionId || !adapter.sendInteractiveInput || !input) return false;
      const generation = terminalGeneration;
      const sessionId = terminal.sessionId;
      terminalInputChain = terminalInputChain.then(() => adapter.sendInteractiveInput!(sessionId, input)).catch((error: unknown) => {
        if (generation === terminalGeneration && state.projectTerminal.sessionId === terminal.sessionId) {
          publish({
            ...state,
            projectTerminal: {
              ...state.projectTerminal,
              message: errorMessage(error, 'Could not send terminal input.'),
            },
          });
        }
      });
      return true;
    },
    resizeProjectTerminal(columns, rows) {
      const terminal = state.projectTerminal;
      if (terminal.status !== 'active' || !terminal.sessionId || !adapter.resizeInteractiveSession
        || !Number.isInteger(columns) || !Number.isInteger(rows)
        || columns < 1 || rows < 1 || columns > 0xffff || rows > 0xffff) return false;
      const generation = terminalGeneration;
      void adapter.resizeInteractiveSession(terminal.sessionId, rows, columns).catch((error: unknown) => {
        if (generation === terminalGeneration && state.projectTerminal.sessionId === terminal.sessionId) {
          publish({
            ...state,
            projectTerminal: {
              ...state.projectTerminal,
              message: errorMessage(error, 'Could not resize the project terminal.'),
            },
          });
        }
      });
      return true;
    },
    async closeProjectTerminal() {
      const terminal = state.projectTerminal;
      if (terminal.status === 'idle' || terminal.status === 'starting' || terminal.status === 'terminating') return false;
      if (terminal.status === 'exited' || terminal.status === 'error' || !terminal.sessionId) {
        terminalGeneration++;
        publish({ ...state, projectTerminal: { status: 'idle', transcript: '' } });
        return true;
      }
      if (!adapter.terminateInteractiveSession) return false;
      const generation = terminalGeneration;
      publish({ ...state, projectTerminal: { ...terminal, status: 'terminating' } });
      try {
        await adapter.terminateInteractiveSession(terminal.sessionId);
        if (generation === terminalGeneration) {
          terminalGeneration++;
          publish({ ...state, projectTerminal: { status: 'idle', transcript: '' } });
        }
        return true;
      } catch (error: unknown) {
        if (generation === terminalGeneration) {
          publish({
            ...state,
            projectTerminal: {
              ...terminal, status: 'active',
              message: errorMessage(error, 'Could not close the project terminal.'),
            },
          });
        }
        return false;
      }
    },
    async restartProjectTerminal() {
      const terminal = state.projectTerminal;
      if (!terminal.project || (terminal.status !== 'exited' && terminal.status !== 'error')) return false;
      return startProjectTerminal(terminal.project, true);
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
      return () => {
        if (request === generation) generation++;
        terminalGeneration++;
        const sessionId = state.projectTerminal.sessionId;
        if (sessionId) void adapter.terminateInteractiveSession?.(sessionId).catch(() => {});
      };
    },
  };
}
