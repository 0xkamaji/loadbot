import type { LoadbotAdapter, LoadbotProject } from '../contract';

// Entirely fictional, in-memory data. Nothing here discovers paths or reads catalogs.
const projects: readonly LoadbotProject[] = [
  {
    catalog: 'personal', tool: 're-toolkit', entries: [
      {
        name: 'Malware triage', path: 'recipes/triage.py', runner: 'python', source: 'catalog',
        description: 'Hashes, strings, file details, reports.',
      },
      {
        name: 'Export functions', path: 'recipes/BinaryNinja/export_functions.py', runner: 'python', source: 'catalog',
        description: 'Export function names from the current analysis database. No input folder is needed.',
      },
      { name: 'Export strings', path: 'recipes/BinaryNinja/print_strings.py', source: 'personal', description: 'Print strings from the current analysis database. This shortcut has no sample inputs.' },
    ],
  },
  {
    catalog: 'personal', tool: 'radio', entries: [
      {
        name: 'Inspect recording', path: 'tools/inspect.py', runner: 'python', source: 'catalog',
        description: 'Inspect a sample recording and its center frequency.',
      },
      { name: 'List devices', path: 'tools/devices.sh', runner: 'bash', source: 'catalog', description: 'A no-input shortcut. Device discovery is not connected in this preview.' },
    ],
  },
  {
    catalog: 'personal', tool: 'rotbot', entries: [
      { name: 'Inspect workspace', path: 'scripts/inspect.py', runner: 'python', source: 'personal', description: 'A fictional project entry, not an integration with Rot.' },
      { name: 'Build report', source: 'personal', description: 'A structured fixture Recipe with ordered parameters.', recipe: {
        version: 1, behavior: 'run', program: { type: 'interpreter', runner: 'python' },
        working_directory: { type: 'project-root' }, arguments: [
          { type: 'project-path', path: 'scripts/report.py' },
          { type: 'input', id: 'format', label: 'Format', kind: 'text', required: true, prefix: '--format' },
          { type: 'switch', id: 'verbose', label: 'Verbose', value: '--verbose', default: false },
        ],
      } },
      { name: 'Open dashboard', source: 'catalog', description: 'A read-only shared Launch Recipe fixture.', recipe: {
        version: 1, behavior: 'launch', program: { type: 'project-file', path: 'bin/dashboard' },
        working_directory: { type: 'target-parent' }, arguments: [],
      } },
    ],
  },
  {
    catalog: 'community', tool: 'research-tools-with-a-long-project-name', entries: [
      { name: 'Export an extended analysis report with full repository-relative paths', path: 'recipes/research/export_extended_analysis_report.py', source: 'catalog', description: 'Long labels wrap without hiding their meaning. Scroll the list to inspect more fixture entries.' },
      ...Array.from({ length: 18 }, (_, index) => ({ name: `Review sample ${String(index + 1).padStart(2, '0')}`, path: `recipes/review_${index + 1}.py`, source: 'catalog' as const })),
    ],
  },
  { catalog: 'community', tool: 're-toolkit', entries: [
    { name: 'Export strings', path: 'export.py', source: 'catalog', description: 'Shared variant; identities include the entry source.' },
    { name: 'Export strings', path: 'export.py', source: 'personal', description: 'Personal variant; this remains independently selectable.' },
  ] },
];

export const fixtureAdapter: LoadbotAdapter = {
  async readInventory() { return structuredClone(projects); },
  async readCatalogs() {
    return [
      { name: 'personal', url: 'fixture://personal', writable: true, state: 'installed', default: true },
      { name: 'community', url: 'fixture://community', writable: false, state: 'installed', default: false },
    ];
  },
  async openProjectFolder() { throw new Error('Folder opening is unavailable in fixture preview.'); },
  async addCatalog() { throw new Error('Management is unavailable in fixture preview.'); },
  async addProject() { throw new Error('Management is unavailable in fixture preview.'); },
  async addShortcut() { throw new Error('Management is unavailable in fixture preview.'); },
  async addRecipeShortcut() { throw new Error('Management is unavailable in fixture preview.'); },
  async updateRecipeShortcut() { throw new Error('Management is unavailable in fixture preview.'); },
  async syncCatalog() { throw new Error('Management is unavailable in fixture preview.'); },
};
