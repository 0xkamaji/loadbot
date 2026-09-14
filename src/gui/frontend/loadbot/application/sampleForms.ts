/** Local UI demonstration configuration. NOT backend input metadata or arguments.
 * A real inventory adapter neither supplies nor depends on these definitions.
 */
export type SampleField =
  | { readonly id: string; readonly kind: 'text'; readonly label: string; readonly required?: boolean; readonly initialValue?: string; readonly placeholder?: string }
  | { readonly id: string; readonly kind: 'path'; readonly label: string; readonly required?: boolean; readonly initialValue?: string; readonly sampleValue: string; readonly pathKind: 'file' | 'folder' }
  | { readonly id: string; readonly kind: 'boolean'; readonly label: string; readonly initialValue?: boolean };

/** Keyed by selectionKey; absent selections have no sample controls. */
export type SampleForms = Readonly<Record<string, readonly SampleField[]>>;
export type SampleValues = Readonly<Record<string, string | boolean>>;
export const noSampleForms: SampleForms = Object.freeze({});

export function initialValues(fields: readonly SampleField[]): SampleValues {
  return Object.fromEntries(fields.map((field) => [field.id, field.initialValue ?? (field.kind === 'boolean' ? false : '')]));
}

export function missingInputs(fields: readonly SampleField[], values: SampleValues): readonly string[] {
  return fields.filter((field) => field.kind !== 'boolean' && field.required && !String(values[field.id] ?? '').trim())
    .map((field) => field.id);
}
