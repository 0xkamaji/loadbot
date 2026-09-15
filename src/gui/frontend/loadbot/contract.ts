/** Semantic inventory projection, not Rust DTOs, a catalog schema, or widget metadata. */
export interface LoadbotProject {
  readonly catalog: string;
  readonly tool: string;
  readonly entries: readonly LoadbotShortcut[];
}

export interface LoadbotShortcut {
  readonly name: string;
  /** Repository-relative location, never an executable command string. */
  readonly path: string;
  readonly description?: string;
  readonly runner?: 'direct' | 'bash' | 'sh' | 'python' | 'powershell';
  readonly source: 'catalog' | 'personal';
}

export type LoadbotRunner = NonNullable<LoadbotShortcut['runner']>;

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
export interface CatalogIdentity { readonly catalog: string }
export interface ProjectIdentity { readonly catalog: string; readonly tool: string }
export interface ShortcutIdentity extends ProjectIdentity { readonly name: string; readonly path: string }

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
  syncCatalog(catalog: string, onActivity?: CatalogSyncActivitySink): Promise<void>;
}
