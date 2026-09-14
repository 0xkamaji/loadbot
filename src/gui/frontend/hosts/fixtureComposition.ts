import { fixtureAdapter } from '../loadbot/fixtures/adapter';
import { fixtureSampleForms } from '../loadbot/fixtures/sampleForms';

// Host composition chooses implementations; neither the view nor controller imports fixtures.
export const fixtureMenuDependencies = { adapter: fixtureAdapter, sampleForms: fixtureSampleForms };
