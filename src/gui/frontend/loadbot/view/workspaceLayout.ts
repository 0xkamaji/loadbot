export interface PaneSizes {
  readonly projects: number;
  readonly shortcuts: number;
  readonly terminal: number;
}

export const defaultPaneSizes: PaneSizes = { projects: 260, shortcuts: 210, terminal: 140 };
export const paneStorageKey = 'loadbot.workspace.panes.v1';

const absoluteLimits: Record<keyof PaneSizes, readonly [number, number]> = {
  projects: [140, 560],
  shortcuts: [110, 640],
  terminal: [88, 440],
};

export function clampPaneSize(name: keyof PaneSizes, value: number): number {
  const [min, max] = absoluteLimits[name];
  return Math.min(max, Math.max(min, value));
}

function storage(): Storage | undefined {
  try { return window.localStorage; } catch { return undefined; }
}

export function restorePaneSizes(source: Pick<Storage, 'getItem'> | undefined = storage()): PaneSizes {
  try {
    const value = JSON.parse(source?.getItem(paneStorageKey) ?? 'null') as unknown;
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

export function persistPaneSizes(value: PaneSizes, target: Pick<Storage, 'setItem'> | undefined = storage()): void {
  try { target?.setItem(paneStorageKey, JSON.stringify({ version: 1, ...value })); } catch { /* presentation preferences are best-effort */ }
}
