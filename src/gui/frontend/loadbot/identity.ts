import type { LoadbotProject, LoadbotShortcut } from './contract';

// Logical identities stay qualified. No installation path or host participates.
export const projectKey = (project: Pick<LoadbotProject, 'catalog' | 'tool'>) => JSON.stringify([project.catalog, project.tool]);
export const shortcutTarget = (shortcut: LoadbotShortcut) => shortcut.path ?? `recipe:${shortcut.recipe?.behavior ?? 'invalid'}`;
export const shortcutKey = (shortcut: LoadbotShortcut) => JSON.stringify([shortcut.source, shortcut.name, shortcutTarget(shortcut)]);
