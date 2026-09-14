import type { MenuAdapter, MenuProject } from './types';

// Entirely fictional, in-memory data. Nothing here discovers paths or reads catalogs.
const projects: readonly MenuProject[] = [
  {
    catalog: 'personal', tool: 're-toolkit', entries: [
      {
        name: 'Malware triage', path: 'recipes/triage.py', runner: 'python', source: 'catalog',
        description: 'Hashes, strings, file details, reports.',
        previewFields: [
          { id: 'input', kind: 'path', label: 'Input folder', pathKind: 'folder', required: true, sampleValue: 'samples/' },
          { id: 'output', kind: 'path', label: 'Output', pathKind: 'folder', initialValue: 'reports/', sampleValue: 'reports/triage/' },
          { id: 'report', kind: 'checkbox', label: 'Open report (sample option)' },
        ],
      },
      {
        name: 'Export functions', path: 'recipes/BinaryNinja/export_functions.py', runner: 'python', source: 'catalog',
        description: 'Export function names from the current analysis database. No input folder is needed.',
        previewFields: [{ id: 'output', kind: 'path', label: 'Output file', pathKind: 'file', initialValue: 'reports/functions.json', sampleValue: 'reports/functions-review.json' }],
      },
      { name: 'Export strings', path: 'recipes/BinaryNinja/print_strings.py', source: 'personal', description: 'Print strings from the current analysis database. This shortcut has no sample inputs.' },
    ],
  },
  {
    catalog: 'personal', tool: 'radio', entries: [
      {
        name: 'Inspect recording', path: 'tools/inspect.py', runner: 'python', source: 'catalog',
        description: 'Inspect a sample recording and its center frequency.',
        previewFields: [
          { id: 'input', kind: 'path', label: 'Recording file', pathKind: 'file', required: true, sampleValue: 'recordings/example.wav' },
          { id: 'frequency', kind: 'text', label: 'Center frequency (MHz)', required: true, placeholder: 'e.g. 100.5' },
        ],
      },
      { name: 'List devices', path: 'tools/devices.sh', runner: 'bash', source: 'catalog', description: 'A no-input shortcut. Device discovery is not connected in this preview.' },
    ],
  },
  {
    catalog: 'personal', tool: 'rotbot', entries: [
      { name: 'Inspect workspace', path: 'scripts/inspect.py', runner: 'python', source: 'personal', description: 'A fictional project entry, not an integration with Rot.' },
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

export const fixtureAdapter: MenuAdapter = {
  mode: 'fixture',
  async readProjects() { return projects; },
};
