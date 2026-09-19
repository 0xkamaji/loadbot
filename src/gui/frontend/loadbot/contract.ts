/** Semantic inventory projection, not Rust DTOs, a catalog schema, or widget metadata. */
export interface LoadbotProject {
  readonly catalog: string;
  readonly tool: string;
  readonly entries: readonly LoadbotShortcut[];
}

export type LoadbotRunner = 'direct' | 'bash' | 'sh' | 'python' | 'powershell';
export type LoadbotInterpreterRunner = Exclude<LoadbotRunner, 'direct'>;

interface LoadbotShortcutFacts {
  readonly name: string;
  readonly description?: string;
  readonly source: 'catalog' | 'personal';
}

export type LoadbotShortcut = LoadbotShortcutFacts & (
  | {
    /** Repository-relative legacy target, never an executable command string. */
    readonly path: string; readonly runner?: LoadbotRunner; readonly recipe?: never;
  }
  | { readonly recipe: LoadbotRecipe; readonly path?: never; readonly runner?: never }
);

export interface LoadbotRecipe {
  readonly version: number;
  readonly behavior: 'run' | 'launch';
  readonly program:
    | { readonly type: 'project-file'; readonly path: string }
    | { readonly type: 'interpreter'; readonly runner: LoadbotInterpreterRunner }
    | { readonly type: 'executable'; readonly name: string };
  readonly working_directory:
    | { readonly type: 'project-root' }
    | { readonly type: 'target-parent' }
    | { readonly type: 'project-relative'; readonly path: string };
  readonly arguments: readonly LoadbotRecipeArgument[];
}

export type LoadbotRecipeArgument =
  | { readonly type: 'project-path'; readonly path: string }
  | { readonly type: 'literal'; readonly value: string }
  | {
    readonly type: 'input'; readonly id: string; readonly label: string;
    readonly kind: 'text' | 'file' | 'directory'; readonly required: boolean;
    readonly default?: string; readonly prefix?: string;
  }
  | {
    readonly type: 'switch'; readonly id: string; readonly label: string;
    readonly value: string; readonly default: boolean;
  };

export interface LoadbotCatalog {
  readonly name: string;
  readonly url: string;
  readonly writable: boolean;
  readonly state: 'missing' | 'installed' | 'mismatch';
  readonly default: boolean;
}

export interface AddCatalogInput { readonly name: string; readonly url: string; readonly writable: boolean }
export interface AddProjectInput {
  readonly catalog: string; readonly name: string; readonly url: string; readonly revision?: string;
  readonly commit: boolean; readonly push: boolean;
}
export interface AddShortcutInput {
  readonly catalog: string; readonly tool: string; readonly name: string; readonly path: string;
  readonly description?: string; readonly runner?: LoadbotRunner;
}
export interface RecipeShortcutInput {
  readonly catalog: string; readonly tool: string; readonly name: string;
  readonly description?: string; readonly recipe: LoadbotRecipe;
}
export interface CatalogIdentity { readonly catalog: string }
export interface ProjectIdentity { readonly catalog: string; readonly tool: string }
export interface ShortcutIdentity extends ProjectIdentity { readonly name: string; readonly path?: string }

export type CatalogSyncStage = 'validating' | 'repository-checked' | 'updating-repository' | 'current' | 'updated';
export interface CatalogSyncActivity {
  readonly stage: CatalogSyncStage;
  readonly catalog: string;
  readonly detail?: string;
}
export type CatalogSyncActivitySink = (activity: CatalogSyncActivity) => void;

/** Backend capabilities are semantic and qualified; native paths never cross this seam.
 * Each read returns a complete, caller-owned inventory snapshot, or rejects.
 */
export interface LoadbotAdapter {
  readInventory(): Promise<readonly LoadbotProject[]>;
  readCatalogs(): Promise<readonly LoadbotCatalog[]>;
  openProjectFolder(project: Pick<LoadbotProject, 'catalog' | 'tool'>): Promise<void>;
  addCatalog(input: AddCatalogInput): Promise<CatalogIdentity>;
  addProject(input: AddProjectInput): Promise<ProjectIdentity>;
  addShortcut(input: AddShortcutInput): Promise<ShortcutIdentity>;
  addRecipeShortcut(input: RecipeShortcutInput): Promise<ShortcutIdentity>;
  updateRecipeShortcut(input: RecipeShortcutInput): Promise<ShortcutIdentity>;
  chooseProjectFile(project: ProjectIdentity): Promise<string | undefined>;
  chooseProjectDirectory(project: ProjectIdentity): Promise<string | undefined>;
  deleteShortcuts(shortcuts: readonly ShortcutIdentity[]): Promise<number>;
  syncCatalog(catalog: string, onActivity?: CatalogSyncActivitySink): Promise<void>;
}
