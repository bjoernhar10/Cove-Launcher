import type { LucideIcon } from "lucide-react";
import { useRef, type KeyboardEvent } from "react";
import { cx } from "../../lib/cx.ts";

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  icon?: LucideIcon;
  /** Show only the icon (label stays the accessible name). */
  iconOnly?: boolean;
}

interface SegmentedProps<T extends string> {
  label: string;
  value: T;
  options: readonly SegmentedOption<T>[];
  onChange: (value: T) => void;
  className?: string;
}

/** Radio group styled as a segmented control. Arrow keys move the selection. */
export function Segmented<T extends string>({ label, value, options, onChange, className }: SegmentedProps<T>) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);

  function onKeyDown(event: KeyboardEvent, index: number): void {
    const delta = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
    if (delta === 0) return;
    event.preventDefault();
    const nextIndex = (index + delta + options.length) % options.length;
    const next = options[nextIndex];
    if (next) {
      onChange(next.value);
      refs.current[nextIndex]?.focus();
    }
  }

  return (
    <div role="radiogroup" aria-label={label} className={cx("inline-flex rounded-md border border-border bg-sidebar p-1", className)}>
      {options.map((option, index) => {
        const selected = option.value === value;
        const Icon = option.icon;
        return (
          <button
            key={option.value}
            ref={(el) => {
              refs.current[index] = el;
            }}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={option.iconOnly ? option.label : undefined}
            title={option.iconOnly ? option.label : undefined}
            tabIndex={selected ? 0 : -1}
            onClick={() => {
              onChange(option.value);
            }}
            onKeyDown={(e) => {
              onKeyDown(e, index);
            }}
            className={cx(
              "inline-flex h-7 items-center gap-2 rounded-md text-sm transition-colors duration-150 ease-out",
              option.iconOnly ? "w-8 justify-center" : "px-3",
              selected ? "border border-border bg-surface font-medium text-fg" : "border border-transparent text-fg-muted hover:text-fg",
            )}
          >
            {Icon && <Icon aria-hidden />}
            {!option.iconOnly && option.label}
          </button>
        );
      })}
    </div>
  );
}
