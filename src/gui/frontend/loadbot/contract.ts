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

/** The only backend capability currently consumed. Selection is application state.
 * Each read returns a complete, caller-owned inventory snapshot, or rejects.
 * No mutation/execution/path-opening capability is exposed in this pass.
 */
export interface LoadbotAdapter {
  readInventory(): Promise<readonly LoadbotProject[]>;
}
