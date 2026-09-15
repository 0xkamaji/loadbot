export interface PaneSizes {
  readonly projects: number;
  readonly shortcuts: number;
  readonly terminal: number;
}

export const defaultPaneSizes: PaneSizes = { projects: 260, shortcuts: 210, terminal: 140 };
export const paneStorageKey = 'loadbot.workspace.panes.v1';

/** Host-owned persistence for one opaque presentation document. */
export interface WorkspaceLayoutStore {
  read(): Promise<string | undefined>;
  write(contents: string): Promise<void>;
}

const absoluteLimits: Record<keyof PaneSizes, readonly [number, number]> = {
  projects: [140, 560],
  shortcuts: [110, 640],
  terminal: [88, 440],
};

export function clampPaneSize(name: keyof PaneSizes, value: number): number {
  const [min, max] = absoluteLimits[name];
  return Math.min(max, Math.max(min, value));
}

function browserStorage(): Storage | undefined {
  try { return window.localStorage; } catch { return undefined; }
}

/** Explicit fixture/browser hosts may retain their layout in the browser origin. */
export const browserWorkspaceLayoutStore: WorkspaceLayoutStore = {
  async read() {
    try { return browserStorage()?.getItem(paneStorageKey) ?? undefined; } catch { return undefined; }
  },
  async write(contents) {
    try { browserStorage()?.setItem(paneStorageKey, contents); } catch { /* preferences are best-effort */ }
  },
};

export function decodePaneSizes(contents: string | undefined): PaneSizes {
  try {
    const value = JSON.parse(contents ?? 'null') as unknown;
    if (!value || typeof value !== 'object' || Array.isArray(value)) return defaultPaneSizes;
    const record = value as Record<string, unknown>;
    if (record.version !== 1) return defaultPaneSizes;
    const restored = { ...defaultPaneSizes };
    for (const name of Object.keys(restored) as (keyof PaneSizes)[]) {
      if (typeof record[name] !== 'number' || !Number.isFinite(record[name])) return defaultPaneSizes;
      restored[name] = clampPaneSize(name, record[name]);
    }
    return restored;
  } catch {
    return defaultPaneSizes;
  }
}

export function encodePaneSizes(value: PaneSizes): string {
  return JSON.stringify({ version: 1, ...value });
}
