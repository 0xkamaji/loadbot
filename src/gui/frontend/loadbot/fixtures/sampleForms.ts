import { selectionKey } from '../identity';
import type { SampleForms } from '../application/sampleForms';

// Separate from inventory: these controls illustrate the UI, not tool capabilities.
// Only the host composition imports this module and injects it into the application.
export const fixtureSampleForms: SampleForms = {
  [selectionKey({ catalog: 'personal', tool: 're-toolkit' }, { name: 'Malware triage', path: 'recipes/triage.py', source: 'catalog' })]: [
    { id: 'input', kind: 'path', label: 'Input folder', pathKind: 'folder', required: true, sampleValue: 'samples/' },
    { id: 'output', kind: 'path', label: 'Output', pathKind: 'folder', initialValue: 'reports/', sampleValue: 'reports/triage/' },
    { id: 'report', kind: 'boolean', label: 'Open report (sample option)' },
  ],
  [selectionKey({ catalog: 'personal', tool: 're-toolkit' }, { name: 'Export functions', path: 'recipes/BinaryNinja/export_functions.py', source: 'catalog' })]: [
    { id: 'output', kind: 'path', label: 'Output file', pathKind: 'file', initialValue: 'reports/functions.json', sampleValue: 'reports/functions-review.json' },
  ],
  [selectionKey({ catalog: 'personal', tool: 'radio' }, { name: 'Inspect recording', path: 'tools/inspect.py', source: 'catalog' })]: [
    { id: 'input', kind: 'path', label: 'Recording file', pathKind: 'file', required: true, sampleValue: 'recordings/example.wav' },
    { id: 'frequency', kind: 'text', label: 'Center frequency (MHz)', required: true, placeholder: 'e.g. 100.5' },
  ],
};
