import type { LoadbotAdapter } from './contract';
import type { SampleForms } from './application/sampleForms';
import { useLoadbotApplication } from './application/useLoadbotApplication';
import type { ShellCallbacks } from '../ui/components';
import { LoadbotMenuView } from './view/LoadbotMenuView';

/** Public composition seam. A parent supplies capabilities and optional UI demos. */
export function LoadbotMenu({ adapter, sampleForms, host, mode = 'local' }: {
  adapter: LoadbotAdapter;
  sampleForms?: SampleForms;
  host?: ShellCallbacks;
  mode?: 'local' | 'fixture';
}) {
  const application = useLoadbotApplication(adapter, mode === 'fixture' ? sampleForms : undefined);
  return <LoadbotMenuView {...application} host={host} mode={mode} />;
}
