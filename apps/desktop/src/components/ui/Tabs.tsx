import { useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import { cx } from "../../lib/cx.ts";

export interface TabItem<T extends string> {
  value: T;
  label: string;
  /** Optional trailing count, e.g. "12/40". */
  meta?: string;
}

interface TabsProps<T extends string> {
  label: string;
  value: T;
  items: readonly TabItem<T>[];
  onChange: (value: T) => void;
  children: ReactNode;
}

/** Horizontal tabs (WAI-ARIA tabs pattern with automatic activation). */
export function Tabs<T extends string>({ label, value, items, onChange, children }: TabsProps<T>) {
  const baseId = useId();
  const refs = useRef<(HTMLButtonElement | null)[]>([]);

  function onKeyDown(event: KeyboardEvent, index: number): void {
    let next = -1;
    if (event.key === "ArrowRight") next = (index + 1) % items.length;
    else if (event.key === "ArrowLeft") next = (index - 1 + items.length) % items.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = items.length - 1;
    if (next < 0) return;
    event.preventDefault();
    const item = items[next];
    if (item) {
      onChange(item.value);
      refs.current[next]?.focus();
    }
  }

  return (
    <div>
      <div role="tablist" aria-label={label} className="flex gap-6 border-b border-border">
        {items.map((item, index) => {
          const selected = item.value === value;
          return (
            <button
              key={item.value}
              ref={(el) => {
                refs.current[index] = el;
              }}
              id={`${baseId}-tab-${item.value}`}
              type="button"
              role="tab"
              aria-selected={selected}
              aria-controls={`${baseId}-panel`}
              tabIndex={selected ? 0 : -1}
              onClick={() => {
                onChange(item.value);
              }}
              onKeyDown={(e) => {
                onKeyDown(e, index);
              }}
              className={cx(
                "-mb-px flex h-10 items-center gap-2 border-b-2 text-sm transition-colors duration-150 ease-out",
                selected ? "border-fg font-medium text-fg" : "border-transparent text-fg-muted hover:text-fg",
              )}
            >
              {item.label}
              {item.meta && <span className="num text-xs text-fg-subtle">{item.meta}</span>}
            </button>
          );
        })}
      </div>
      <div role="tabpanel" id={`${baseId}-panel`} aria-labelledby={`${baseId}-tab-${value}`} className="pt-6">
        {children}
      </div>
    </div>
  );
}
