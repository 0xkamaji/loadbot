import { useId, useRef, type ButtonHTMLAttributes, type HTMLAttributes, type InputHTMLAttributes, type KeyboardEvent, type ReactNode } from 'react';
import { asset, themeStyle } from './theme';
import './theme.css';

export interface ShellCallbacks {
  onClose?: () => void;
}

export function ApplicationFrame({ label, children }: { label: string; children: ReactNode }) {
  return <section className="lb-theme lb-window" style={themeStyle} aria-label={label}>{children}</section>;
}

export function Panel({ className = '', ...props }: HTMLAttributes<HTMLElement>) {
  return <section className={`lb-panel ${className}`} {...props} />;
}

export type IconName = 'close' | 'folder' | 'arrow-right' | 'terminal' | 'check' | 'chevron-down';
export function Icon({ name, inverse = false }: { name: IconName; inverse?: boolean }) {
  return <img className="lb-icon" src={asset(`loadbot.icons${inverse ? '-inverse' : ''}.${name}`)} alt="" />;
}

export function Button({ className = '', children, ...props }: ButtonHTMLAttributes<HTMLButtonElement>) {
  return <button type="button" className={`lb-button ${className}`} {...props}>{children}</button>;
}

export function IconButton({ icon, label, ...props }: Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> & { icon: IconName; label: string }) {
  return <Button {...props} className="lb-icon-button" aria-label={label} title={label}><Icon name={icon} /></Button>;
}

export function MenuRow({ selected, icon, children, ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { selected: boolean; icon: IconName }) {
  return <button type="button" className="lb-menu-row" aria-pressed={selected} {...props}>
    <Icon name={icon} inverse={selected} /><span>{children}</span>
  </button>;
}

// Native buttons provide Tab/Shift+Tab, Enter and Space. Arrow/Home/End move focus
// without changing selection: users explicitly activate the focused row.
export function MenuList({ label, children }: { label: string; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  function navigate(event: KeyboardEvent<HTMLDivElement>) {
    const buttons = Array.from(ref.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? []);
    const index = buttons.indexOf(event.target as HTMLButtonElement);
    if (index < 0) return;
    let next: number;
    switch (event.key) {
      case 'ArrowDown': next = Math.min(index + 1, buttons.length - 1); break;
      case 'ArrowUp': next = Math.max(index - 1, 0); break;
      case 'Home': next = 0; break;
      case 'End': next = buttons.length - 1; break;
      default: return;
    }
    event.preventDefault();
    buttons[next]?.focus();
  }
  return <div className="lb-menu-list" role="group" aria-label={label} ref={ref} onKeyDown={navigate}>{children}</div>;
}

export function InputControl({ label, id, ...props }: InputHTMLAttributes<HTMLInputElement> & { label: string }) {
  const generatedId = useId();
  const inputId = id ?? generatedId;
  return <label className="lb-field" htmlFor={inputId}>
    <span>{label}{props.required && <span aria-hidden="true"> *</span>}</span>
    <span className="lb-input-frame"><input className="lb-input" id={inputId} {...props} /></span>
  </label>;
}

export function PathSelector({ action, ...input }: InputHTMLAttributes<HTMLInputElement> & {
  label: string; action: ReactNode;
}) {
  return <div className="lb-path-selector">
    <InputControl {...input} />
    {action}
  </div>;
}

export function Checkbox({ label, ...props }: Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> & { label: string }) {
  return <label className="lb-checkbox"><input type="checkbox" {...props} /><span>{label}</span></label>;
}

export function StatusDisplay({ children, id }: { children: ReactNode; id?: string }) {
  return <p className="lb-status" id={id} role="status" aria-live="polite">{children}</p>;
}

export function BottomDrawer({ open, id, label, children }: { open: boolean; id: string; label: string; children: ReactNode }) {
  return <section id={id} className="lb-drawer" aria-label={label} hidden={!open} tabIndex={0}>
    {children}
  </section>;
}
