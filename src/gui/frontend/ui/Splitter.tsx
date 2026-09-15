import { useRef, type KeyboardEvent, type PointerEvent } from 'react';

export interface SplitLimits { min: number; max: number }

export function clampSplit(value: number, limits: SplitLimits): number {
  const min = Number.isFinite(limits.min) ? limits.min : 0;
  const max = Number.isFinite(limits.max) ? Math.max(min, limits.max) : min;
  return Math.min(max, Math.max(min, value));
}

/** Generic two-pane boundary. The owning layout supplies dimensions and persistence. */
export function Splitter({ orientation, label, value, limits, direction = 1, onChange, onCommit, onReset }: {
  orientation: 'vertical' | 'horizontal';
  label: string;
  value: number;
  limits: () => SplitLimits;
  direction?: 1 | -1;
  onChange(value: number): void;
  onCommit(value: number): void;
  onReset(): void;
}) {
  const drag = useRef<{ pointer: number; start: number; value: number; latest: number } | undefined>(undefined);
  const currentLimits = limits();
  const minimum = Number.isFinite(currentLimits.min) ? currentLimits.min : 0;
  const maximum = Number.isFinite(currentLimits.max) ? Math.max(minimum, currentLimits.max) : minimum;
  const coordinate = (event: PointerEvent) => orientation === 'vertical' ? event.clientX : event.clientY;
  function move(event: PointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || current.pointer !== event.pointerId) return;
    event.preventDefault();
    current.latest = clampSplit(current.value + (coordinate(event) - current.start) * direction, limits());
    onChange(current.latest);
  }
  function finish(event: PointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || current.pointer !== event.pointerId) return;
    drag.current = undefined;
    onCommit(current.latest);
  }
  function keyboard(event: KeyboardEvent<HTMLDivElement>) {
    const axisKey = orientation === 'vertical'
      ? event.key === 'ArrowLeft' ? -1 : event.key === 'ArrowRight' ? 1 : 0
      : event.key === 'ArrowUp' ? -1 : event.key === 'ArrowDown' ? 1 : 0;
    if (!axisKey) return;
    event.preventDefault();
    const next = clampSplit(value + axisKey * 12 * direction, limits());
    onChange(next);
    onCommit(next);
  }
  return <div
    className={`lb-splitter lb-splitter-${orientation}`}
    role="separator"
    aria-label={label}
    aria-orientation={orientation}
    aria-valuemin={Math.round(minimum)}
    aria-valuemax={Math.round(maximum)}
    aria-valuenow={Math.round(value)}
    tabIndex={0}
    onDoubleClick={onReset}
    onKeyDown={keyboard}
    onPointerDown={(event) => {
      if (event.button !== 0) return;
      event.preventDefault();
      event.currentTarget.setPointerCapture?.(event.pointerId);
      drag.current = { pointer: event.pointerId, start: coordinate(event), value, latest: value };
    }}
    onPointerMove={move}
    onPointerUp={finish}
    onPointerCancel={finish}
  />;
}
