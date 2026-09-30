import type {
  AddCatalogInput, AddProjectInput, CatalogConnectActivityEvent, CatalogConnectStage, CatalogSyncActivityEvent, CatalogSyncStage,
  CatalogDeletionPlan, ConnectCatalogInput, CreateCatalogInput, LoadbotAdapter, LoadbotCatalog,
  InteractiveLaunch, InteractiveSessionEvent, LoadbotProject, OperationLogActivity, ProjectOperationActivityEvent, ProjectOperationStage,
  ProjectDirectoryListing, RepositoryChange,
} from '../contract';
import { projectKey } from '../identity';
import { completeLoadbotCommand, executeLoadbotCommand, type CommandCompletion, type CommandResult } from './command';

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
export type CatalogLifecycleAction = 'unregister' | 'delete-local' | 'delete-with-managed-tools';
export type ManagementKind = 'add-catalog' | 'create-catalog' | 'connect-catalog' | 'add-project' | 'sync-catalog'
  | 'unregister-catalog' | 'delete-local-catalog' | 'delete-catalog-with-managed-tools'
  | 'pull-project' | 'push-project' | 'update-project' | 'remove-project' | 'reinstall-project';
export type ManagementState =
  | { readonly status: 'idle' }
  | { readonly status: 'submitting'; readonly kind: ManagementKind; readonly message: string }
  | { readonly status: 'success'; readonly kind: ManagementKind; readonly message: string }
  | { readonly status: 'cancelled'; readonly kind: ManagementKind; readonly message: string }
  | { readonly status: 'error'; readonly kind: ManagementKind; readonly message: string };
export type ActivityOperation = 'catalog-sync' | 'catalog-add' | 'catalog-create' | 'catalog-connect' | 'project-add' | 'local-reload'
  | 'catalog-unregister' | 'catalog-delete-local' | 'catalog-delete-managed'
  | 'project-folder-open' | 'project-pull' | 'project-push' | 'project-update' | 'project-remove' | 'project-reinstall';
export type ActivityStatus = 'in-progress' | 'info' | 'success' | 'error' | 'cancelled';
export type ActivityStage = CatalogSyncStage | CatalogConnectStage | ProjectOperationStage | 'started' | 'interactive-authentication'
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
  readonly catalog?: { readonly catalog: string };
  readonly transcript: string;
  readonly launchId?: string;
  readonly sessionId?: string;
  readonly processId?: string;
  readonly exitCode?: number;
  readonly signal?: string;
  readonly cancelled?: boolean;
  readonly message?: string;
}
export interface LoadbotState {
  readonly inventory: InventoryState;
  readonly catalogState: CatalogState;
  readonly currentCatalog?: string;
  readonly projectFilter: ProjectFilter;
  readonly project?: LoadbotProject;
  readonly drawerOpen: boolean;
  readonly bottomView: 'command' | 'activity' | 'terminal';
  readonly command: CommandState;
  readonly activity: readonly ActivityEntry[];
  readonly activityLogs: readonly ActivityLogEntry[];
  readonly management: ManagementState;
  readonly projectFolder: {
    readonly status: 'idle' | 'opening' | 'opened' | 'error';
    readonly projectId?: string;
    readonly message?: string;
  };
  readonly projectFiles: {
    readonly status: 'idle' | 'loading' | 'ready' | 'error';
    readonly projectId?: string;
    readonly relativePath?: string;
    readonly listing?: ProjectDirectoryListing;
    readonly message?: string;
  };
  readonly catalogFolder: {
    readonly status: 'idle' | 'opening' | 'opened' | 'error';
    readonly catalog?: string;
    readonly message?: string;
  };
  readonly projectTerminal: ProjectTerminalState;
  readonly pendingProjectAction?: { readonly action: ProjectLifecycleAction; readonly project: LoadbotProject };
  readonly pendingCatalogAction?: {
    readonly action: CatalogLifecycleAction;
    readonly catalog: string;
    readonly plan?: CatalogDeletionPlan;
  };
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
  reloadInventory(): void;
  openProjectFolder(id: string): void;
  openProjectTerminal(id: string): void;
  openCatalogFolder(): void;
  openCatalogTerminal(): void;
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
  requestCatalogAction(action: CatalogLifecycleAction): void;
  cancelCatalogAction(): void;
  confirmCatalogAction(): Promise<boolean>;
  addCatalog(input: AddCatalogInput): Promise<boolean>;
  createCatalog(input: CreateCatalogInput): Promise<boolean>;
  connectCatalog(input: ConnectCatalogInput): Promise<boolean>;
  addProject(input: Omit<AddProjectInput, 'catalog'>): Promise<boolean>;
  readProjectDirectory(relativePath?: string): Promise<boolean>;
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
  toggleDrawer(): void;
}

interface ReadPreference { catalog?: string; projectId?: string; projectFilter?: ProjectFilter }

/** Deterministic application state, independent of React, DOM, themes and hosts.
 * Backend-confirmed writes are always followed by an authoritative workspace read.
 */
export function createLoadbotApplication(adapter: LoadbotAdapter) {
  let state: LoadbotState = {
    inventory: { status: 'loading' }, catalogState: { status: 'loading' }, projectFilter: 'installed', drawerOpen: true, bottomView: 'command',
    command: { entries: [], history: [], interactiveTranscript: [] },
    activity: [], activityLogs: [], management: { status: 'idle' },
    projectFolder: { status: 'idle' }, projectFiles: { status: 'idle' }, catalogFolder: { status: 'idle' }, projectTerminal: { status: 'idle', transcript: '' },
  };
  const listeners = new Set<() => void>();
  let generation = 0;
  let folderGeneration = 0;
  let filesGeneration = 0;
  let catalogFolderGeneration = 0;
  let terminalGeneration = 0;
  let terminalInputChain = Promise.resolve();
  let activityId = 0;
  let operationId = 0;
  let activityLogId = 0;
  let commandId = 0;
  let interactiveTranscriptId = 0;
  let interactiveGeneration = 0;
  const pendingInteractiveLaunches: InteractiveLaunch[] = [];
  const publish = (next: LoadbotState) => {
    state = next;
    listeners.forEach((listener) => listener());
  };
  const selection = (project?: LoadbotProject) => ({ project });
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

  async function beginWorkspaceRead(preferred: ReadPreference = {}): Promise<boolean> {
    const request = ++generation;
    const selectedProject = preferred.projectId ?? (state.project && projectKey(state.project));
    const selectedCatalog = preferred.catalog ?? state.currentCatalog;
    const projectFilter = preferred.projectFilter ?? state.projectFilter;
    folderGeneration++;
    filesGeneration++;
    publish({ ...state, inventory: { status: 'loading' }, catalogState: { status: 'loading' }, projectFilter, ...selection(), projectFolder: { status: 'idle' }, projectFiles: { status: 'idle' } });
    try {
      const [projects, catalogs] = await Promise.all([adapter.readInventory(), adapter.readCatalogs()]);
      if (request !== generation) return false;
      const catalogNames = new Set(catalogs.map((catalog) => catalog.name));
      const currentCatalog = selectedCatalog && (catalogNames.has(selectedCatalog) || projects.some((project) => project.catalog === selectedCatalog))
        ? selectedCatalog
        : catalogs.find((catalog) => catalog.default)?.name ?? projects[0]?.catalog ?? catalogs[0]?.name;
      const visible = projectsFor(projects, currentCatalog, projectFilter);
      const project = visible.find((item) => projectKey(item) === selectedProject) ?? visible[0];
      publish({
        ...state, inventory: { status: 'ready', projects }, catalogState: { status: 'ready', catalogs }, currentCatalog,
        projectFilter, ...selection(project), projectFiles: { status: 'idle' },
      });
      return true;
    } catch (error: unknown) {
      if (request === generation) publish({
        ...state,
        inventory: { status: 'error', message: error instanceof Error ? error.message : undefined },
        catalogState: { status: 'error', message: error instanceof Error ? error.message : undefined },
        ...selection(), projectFolder: { status: 'idle' }, projectFiles: { status: 'idle' },
      });
      return false;
    }
  }

  async function mutation(
    kind: ManagementKind,
    activityOperation: ActivityOperation,
    progress: string,
    completed: string,
    context: Pick<ActivityEntry, 'catalog' | 'project'>,
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

  type TerminalTarget =
    | { readonly kind: 'project'; readonly catalog: string; readonly tool: string }
    | { readonly kind: 'catalog'; readonly catalog: string };
  const terminalTarget = (terminal: ProjectTerminalState): TerminalTarget | undefined => terminal.project
    ? { kind: 'project', ...terminal.project }
    : terminal.catalog ? { kind: 'catalog', ...terminal.catalog } : undefined;
  const sameTerminalTarget = (left: TerminalTarget | undefined, right: TerminalTarget) => left?.kind === right.kind
    && left.catalog === right.catalog && (left.kind !== 'project' || right.kind !== 'project' || left.tool === right.tool);
  const boundedTerminalTranscript = (current: string, text: string) =>
    `${current}${text}`.slice(-TERMINAL_TRANSCRIPT_LIMIT);

  async function startTerminal(
    target: TerminalTarget,
    restart = false,
  ): Promise<boolean> {
    const current = state.projectTerminal;
    if (!restart && terminalTarget(current)) {
      return sameTerminalTarget(terminalTarget(current), target) && current.status === 'active';
    }
    const createLaunch = target.kind === 'project' ? adapter.createProjectTerminalLaunch : adapter.createCatalogTerminalLaunch;
    if (!createLaunch || !adapter.startInteractiveSession
      || !adapter.sendInteractiveInput || !adapter.terminateInteractiveSession) {
      publish({
        ...state,
        projectTerminal: {
          status: 'error', ...(target.kind === 'project'
            ? { project: { catalog: target.catalog, tool: target.tool } }
            : { catalog: { catalog: target.catalog } }), transcript: '',
          message: 'Embedded terminals are unavailable in this host.',
        },
      });
      return false;
    }
    const generation = ++terminalGeneration;
    publish({
      ...state,
      projectTerminal: { status: 'starting', ...(target.kind === 'project'
        ? { project: { catalog: target.catalog, tool: target.tool } }
        : { catalog: { catalog: target.catalog } }), transcript: '' },
    });
    try {
      const launch = target.kind === 'project'
        ? await adapter.createProjectTerminalLaunch!({ catalog: target.catalog, tool: target.tool })
        : await adapter.createCatalogTerminalLaunch!({ catalog: target.catalog });
      if (generation !== terminalGeneration) return false;
      publish({
        ...state,
        projectTerminal: { ...state.projectTerminal, launchId: launch.launchId },
      });
      const onEvent = (event: InteractiveSessionEvent) => {
        if (generation !== terminalGeneration) return;
        const terminal = state.projectTerminal;
        if (!sameTerminalTarget(terminalTarget(terminal), target)) return;
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
            ...state.projectTerminal, status: 'error',
            ...(target.kind === 'project'
              ? { project: { catalog: target.catalog, tool: target.tool } }
              : { catalog: { catalog: target.catalog } }),
            message: errorMessage(error, 'Could not start the terminal.'),
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
      filesGeneration++;
      publish({ ...state, currentCatalog: name, ...selection(projects[0]), projectFolder: { status: 'idle' }, projectFiles: { status: 'idle' } });
    },
    selectProjectFilter(filter) {
      if (filter === state.projectFilter) return;
      const projects = state.inventory.status === 'ready' ? projectsFor(state.inventory.projects, state.currentCatalog, filter) : [];
      const selectedId = state.project && projectKey(state.project);
      const current = selectedId ? projects.find((project) => projectKey(project) === selectedId) : undefined;
      folderGeneration++;
      filesGeneration++;
      publish({ ...state, projectFilter: filter, ...selection(current ?? projects[0]), projectFolder: { status: 'idle' }, projectFiles: { status: 'idle' } });
    },
    selectProject(id) {
      if (state.inventory.status !== 'ready') return;
      const project = projectsFor(state.inventory.projects, state.currentCatalog).find((item) => projectKey(item) === id);
      if (!project || project === state.project) return;
      folderGeneration++;
      filesGeneration++;
      publish({ ...state, ...selection(project), projectFolder: { status: 'idle' }, projectFiles: { status: 'idle' } });
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
      if (project.installed !== false && !terminalTarget(state.projectTerminal)) {
        void startTerminal({ kind: 'project', catalog: project.catalog, tool: project.tool });
      }
    },
    openCatalogFolder() {
      const catalog = state.currentCatalog;
      const available = state.catalogState.status === 'ready'
        && state.catalogState.catalogs.some((item) => item.name === catalog && item.state === 'installed');
      if (!catalog || !available) return;
      const request = ++catalogFolderGeneration;
      publish({ ...state, catalogFolder: { status: 'opening', catalog } });
      Promise.resolve().then(() => adapter.openCatalogFolder({ catalog })).then(
        () => {
          if (request === catalogFolderGeneration) {
            publish({ ...state, catalogFolder: { status: 'opened', catalog, message: `Opened ${catalog}.` } });
          }
        },
        (error: unknown) => {
          if (request === catalogFolderGeneration) {
            publish({
              ...state,
              catalogFolder: { status: 'error', catalog, message: errorMessage(error, 'Could not open the catalog folder.') },
            });
          }
        },
      );
    },
    openCatalogTerminal() {
      const catalog = state.currentCatalog;
      const available = state.catalogState.status === 'ready'
        && state.catalogState.catalogs.some((item) => item.name === catalog && item.state === 'installed');
      if (!catalog || !available) return;
      publish({ ...state, bottomView: 'terminal', drawerOpen: true });
      if (!terminalTarget(state.projectTerminal)) void startTerminal({ kind: 'catalog', catalog });
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
    async createCatalog(input) {
      return mutation('create-catalog', 'catalog-create', `Creating catalog ${input.name}…`, `Catalog ${input.name} created.`, { catalog: input.name }, async () => {
        const created = await adapter.createCatalog(input);
        return { catalog: created.catalog };
      });
    },
    async connectCatalog(input) {
      const catalog = state.catalogState.status === 'ready'
        ? state.catalogState.catalogs.find((item) => item.name === input.name) : undefined;
      if (!catalog || catalog.backend !== 'local' || !catalog.writable || catalog.state !== 'installed' || !adapter.connectCatalog) return false;
      return mutation('connect-catalog', 'catalog-connect', `Connecting catalog ${catalog.name}…`, `Catalog ${catalog.name} connected to Git.`, { catalog: catalog.name }, async (operation) => {
        const connected = await adapter.connectCatalog!(input, (activity: CatalogConnectActivityEvent) => {
          if ('kind' in activity && activity.kind === 'log') appendLog(operation, activity);
          else if ('kind' in activity && activity.kind === 'interactive-launch') {
            appendActivity({
              operationId: operation, operation: 'catalog-connect', stage: 'interactive-authentication',
              status: 'in-progress', catalog: catalog.name, detail: 'Interactive authentication required.',
            });
            if (state.command.interactive) pendingInteractiveLaunches.push(activity);
            else void actions.startInteractiveSession(activity);
          }
          else appendActivity({
            operationId: operation, operation: 'catalog-connect', stage: activity.stage,
            status: 'in-progress', catalog: activity.catalog,
          });
        });
        return { catalog: connected.catalog };
      });
    },
    requestCatalogAction(action) {
      const catalog = state.currentCatalog;
      if (!catalog || state.management.status === 'submitting') return;
      const kind: ManagementKind = action === 'unregister' ? 'unregister-catalog'
        : action === 'delete-local' ? 'delete-local-catalog' : 'delete-catalog-with-managed-tools';
      if (action === 'unregister') {
        if (!adapter.unregisterCatalog) return;
        publish({ ...state, pendingCatalogAction: { action, catalog }, management: { status: 'idle' } });
        return;
      }
      const inspect = action === 'delete-local' ? adapter.inspectLocalCatalogDeletion : adapter.inspectCatalogDeletion;
      if (!inspect || (action === 'delete-local' ? !adapter.deleteLocalCatalog : !adapter.deleteCatalogWithManagedTools)) return;
      publish({
        ...state,
        pendingCatalogAction: { action, catalog },
        management: { status: 'submitting', kind, message: `Inspecting catalog ${catalog}…` },
      });
      void inspect.call(adapter, { catalog }).then(
        (plan) => {
          if (state.pendingCatalogAction?.catalog !== catalog || state.pendingCatalogAction.action !== action) return;
          if (plan.catalog !== catalog) {
            publish({
              ...state,
              management: { status: 'error', kind, message: 'The backend returned a deletion plan for a different catalog.' },
            });
            return;
          }
          publish({ ...state, pendingCatalogAction: { action, catalog, plan }, management: { status: 'idle' } });
        },
        (error: unknown) => {
          if (state.pendingCatalogAction?.catalog !== catalog || state.pendingCatalogAction.action !== action) return;
          publish({
            ...state,
            management: { status: 'error', kind, message: errorMessage(error, 'Could not inspect catalog deletion.') },
          });
        },
      );
    },
    cancelCatalogAction() {
      if (state.management.status !== 'submitting') {
        publish({ ...state, pendingCatalogAction: undefined, management: { status: 'idle' } });
      }
    },
    async confirmCatalogAction() {
      const pending = state.pendingCatalogAction;
      if (!pending || state.management.status === 'submitting') return false;
      const { action, catalog } = pending;
      if (action !== 'unregister' && !pending.plan) return false;
      const capability = action === 'unregister' ? adapter.unregisterCatalog
        : action === 'delete-local' ? adapter.deleteLocalCatalog : adapter.deleteCatalogWithManagedTools;
      if (!capability) return false;
      const kind: ManagementKind = action === 'unregister' ? 'unregister-catalog'
        : action === 'delete-local' ? 'delete-local-catalog' : 'delete-catalog-with-managed-tools';
      const activity: ActivityOperation = action === 'unregister' ? 'catalog-unregister'
        : action === 'delete-local' ? 'catalog-delete-local' : 'catalog-delete-managed';
      const progress = action === 'unregister' ? `Unregistering catalog ${catalog}…`
        : action === 'delete-local' ? `Deleting local catalog ${catalog}…`
          : `Deleting catalog ${catalog} and its managed tools…`;
      const completed = action === 'unregister' ? `Catalog ${catalog} unregistered.`
        : action === 'delete-local' ? `Local catalog ${catalog} deleted.`
          : `Catalog ${catalog} and its managed tools deleted.`;
      const result = await mutation(kind, activity, progress, completed, { catalog }, async () => {
        const changed = await capability.call(adapter, { catalog });
        if (changed.catalog !== catalog) throw new Error('The backend returned a different catalog identity.');
        return {};
      });
      if (result) publish({ ...state, pendingCatalogAction: undefined });
      else if (action !== 'unregister' && state.pendingCatalogAction?.catalog === catalog) {
        publish({ ...state, pendingCatalogAction: { action, catalog } });
      }
      return result;
    },
    async addProject(input) {
      const catalog = state.currentCatalog;
      if (!catalog) return false;
      return mutation('add-project', 'project-add', `Adding project ${input.name}…`, `Project ${input.name} added.`, { catalog, project: input.name }, async () => {
        const created = await adapter.addProject({ ...input, catalog });
        return { catalog: created.catalog, projectId: projectKey(created) };
      });
    },
    async readProjectDirectory(relativePath = '') {
      const project = state.project;
      if (!project || project.installed === false) return false;
      const identity = { catalog: project.catalog, tool: project.tool };
      const id = projectKey(project);
      if (!adapter.readProjectDirectory) {
        publish({ ...state, projectFiles: { status: 'error', projectId: id, message: 'Project files are unavailable in this host.' } });
        return false;
      }
      const request = ++filesGeneration;
      publish({ ...state, projectFiles: { status: 'loading', projectId: id, relativePath } });
      try {
        const listing = await adapter.readProjectDirectory(identity, relativePath);
        if (request !== filesGeneration || !state.project || projectKey(state.project) !== id) return false;
        publish({ ...state, projectFiles: { status: 'ready', projectId: id, listing } });
        return true;
      } catch (error: unknown) {
        if (request !== filesGeneration || !state.project || projectKey(state.project) !== id) return false;
        publish({
          ...state,
          projectFiles: { status: 'error', projectId: id, relativePath, message: errorMessage(error, 'Could not read project files.') },
        });
        return false;
      }
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
      if (view === 'terminal' && state.project?.installed !== false && !terminalTarget(state.projectTerminal) && state.project) {
        void startTerminal({ kind: 'project', catalog: state.project.catalog, tool: state.project.tool });
      }
    },
    completeCommand(input, caret) {
      if (state.command.interactive) return undefined;
      const projects = state.inventory.status === 'ready' ? state.inventory.projects : [];
      return completeLoadbotCommand(input, caret, {
        inventoryStatus: state.inventory.status,
        projects,
        currentCatalog: state.currentCatalog,
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
      const target = terminalTarget(terminal);
      if (!target || (terminal.status !== 'exited' && terminal.status !== 'error')) return false;
      return startTerminal(target, true);
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
