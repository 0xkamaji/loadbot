import { fixtureAdapter } from '../loadbot/fixtures/adapter';

// Host composition chooses implementations; neither the view nor controller imports fixtures.
export const fixtureMenuDependencies = { adapter: fixtureAdapter, mode: 'fixture' as const };
