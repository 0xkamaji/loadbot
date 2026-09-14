import type { LoadbotAdapter, LoadbotProject, LoadbotShortcut } from '../contract';
import { projectKey, selectionKey, shortcutKey } from '../identity';
import { initialValues, missingInputs, noSampleForms, type SampleField, type SampleForms, type SampleValues } from './sampleForms';

export type InventoryState =
  | { readonly status: 'loading' }
  | { readonly status: 'ready'; readonly projects: readonly LoadbotProject[] }
  | { readonly status: 'error'; readonly message?: string };

export interface LoadbotState {
  readonly inventory: InventoryState;
  readonly project?: LoadbotProject;
  readonly shortcut?: LoadbotShortcut;
  readonly fields: readonly SampleField[];
  readonly values: SampleValues;
  readonly missingInputIds: readonly string[];
  readonly drawerOpen: boolean;
}

export interface LoadbotActions {
  selectProject(id: string): void;
  selectShortcut(id: string): void;
  changeSampleInput(id: string, value: string | boolean): void;
  useSamplePath(id: string): void;
  toggleDrawer(): void;
}

/** Deterministic application state, independent of React, DOM, themes and hosts.
 * Subscribers observe state; they do not manufacture operation results.
 */
export function createLoadbotApplication(adapter: LoadbotAdapter, sampleForms: SampleForms = noSampleForms) {
  let state: LoadbotState = {
    inventory: { status: 'loading' }, fields: [], values: {}, missingInputIds: [], drawerOpen: false,
  };
  const listeners = new Set<() => void>();
  let generation = 0;
  const publish = (next: LoadbotState) => {
    state = next;
    listeners.forEach((listener) => listener());
  };
  const selection = (project?: LoadbotProject, shortcut = project?.entries[0]) => {
    const fields = project && shortcut ? sampleForms[selectionKey(project, shortcut)] ?? [] : [];
    const values = initialValues(fields);
    return { project, shortcut, fields, values, missingInputIds: missingInputs(fields, values) };
  };
  function changeSampleInput(id: string, value: string | boolean) {
    const field = state.fields.find((item) => item.id === id);
    if (!field || (field.kind === 'boolean' ? typeof value !== 'boolean' : typeof value !== 'string')) return;
    const values = { ...state.values, [id]: value };
    publish({ ...state, values, missingInputIds: missingInputs(state.fields, values) });
  }
  const actions: LoadbotActions = {
    selectProject(id) {
      if (state.inventory.status !== 'ready') return;
      const project = state.inventory.projects.find((item) => projectKey(item) === id);
      if (!project || project === state.project) return;
      publish({ ...state, ...selection(project) });
    },
    selectShortcut(id) {
      const shortcut = state.project?.entries.find((item) => shortcutKey(item) === id);
      if (!shortcut || shortcut === state.shortcut) return;
      publish({ ...state, ...selection(state.project, shortcut) });
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
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    actions,
    /** Host lifecycle read, not catalog synchronization. Cleanup ignores late results;
     * it does not claim to cancel an adapter's underlying work.
     */
    start() {
      const request = ++generation;
      publish({ ...state, inventory: { status: 'loading' }, ...selection() });
      Promise.resolve().then(() => adapter.readInventory()).then(
        (projects) => {
          if (request === generation) publish({ ...state, inventory: { status: 'ready', projects }, ...selection(projects[0]) });
        },
        (error: unknown) => {
          if (request === generation) publish({
            ...state, inventory: { status: 'error', message: error instanceof Error ? error.message : undefined }, ...selection(),
          });
        },
      );
      return () => { if (request === generation) generation++; };
    },
  };
}
