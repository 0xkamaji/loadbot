import { StrictMode, useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { fixtureMenuDependencies } from './fixtureComposition';
import { LoadbotMenu } from '../loadbot/LoadbotMenu';
import './host.css';

function EmbeddingExample() {
  const [open, setOpen] = useState(true);
  const dialog = useRef<HTMLDialogElement>(null);
  const launcher = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (open) dialog.current?.showModal();
    else { dialog.current?.close(); launcher.current?.focus(); }
  }, [open]);
  return <main className="embedding-page">
    <h1>Parent application example</h1>
    <p>This browser-only parent owns the overlay, focus containment, Escape, and close action.</p>
    <button ref={launcher} onClick={() => setOpen(true)}>Open Loadbot overlay</button>
    <dialog ref={dialog} className="embedding-dialog" aria-label="Loadbot overlay" onCancel={(event) => { event.preventDefault(); setOpen(false); }}>
      <div className="embedding-container">
        {open && <LoadbotMenu {...fixtureMenuDependencies} host={{ onClose: () => setOpen(false) }} />}
      </div>
    </dialog>
  </main>;
}

if (import.meta.env.DEV) {
  createRoot(document.getElementById('root')!).render(<StrictMode><EmbeddingExample /></StrictMode>);
}
