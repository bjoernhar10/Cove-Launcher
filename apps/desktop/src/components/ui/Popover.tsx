import { ChevronDown } from "lucide-react";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { cx } from "../../lib/cx.ts";

interface PopoverProps {
  label: string;
  /** Text shown on the trigger, e.g. the current selection. */
  value?: string | undefined;
  active?: boolean;
  children: ReactNode;
  align?: "start" | "end";
  /** Panel width class, default min-w-52. */
  panelClassName?: string;
}

/** Filter-style dropdown trigger with a panel. Closes on Esc and outside click. */
export function Popover({ label, value, active = false, children, align = "start", panelClassName = "min-w-52" }: PopoverProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelId = useId();

  useEffect(() => {
    if (!open) return;
    function onPointerDown(event: PointerEvent): void {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) setOpen(false);
    }
    function onKeyDown(event: KeyboardEvent): void {
      if (event.key === "Escape") {
        setOpen(false);
        triggerRef.current?.focus();
      }
    }
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    rootRef.current?.querySelector<HTMLElement>("[data-popover-panel] input, [data-popover-panel] button")?.focus();
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="relative">
      <button
        ref={triggerRef}
        type="button"
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => {
          setOpen((o) => !o);
        }}
        className={cx(
          "inline-flex h-8 items-center gap-2 rounded-md border px-3 text-sm transition-colors duration-150 ease-out",
          active ? "border-border-strong bg-active text-fg" : "border-border bg-surface text-fg-muted hover:bg-hover hover:text-fg",
        )}
      >
        <span>{label}</span>
        {value && <span className="max-w-32 truncate font-medium text-fg">{value}</span>}
        <ChevronDown aria-hidden className="text-fg-subtle" />
      </button>
      {open && (
        <div
          id={panelId}
          data-popover-panel
          className={cx(
            "absolute top-full z-30 mt-1 rounded-lg border border-border bg-raised p-1",
            panelClassName,
            align === "end" ? "right-0" : "left-0",
          )}
        >
          {children}
        </div>
      )}
    </div>
  );
}

interface CheckOptionProps {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

export function CheckOption({ label, checked, onChange }: CheckOptionProps) {
  return (
    <label className="flex h-8 cursor-pointer items-center gap-3 rounded-md px-2 text-sm text-fg hover:bg-hover">
      <input
        type="checkbox"
        className="size-4"
        checked={checked}
        onChange={(e) => {
          onChange(e.target.checked);
        }}
      />
      {label}
    </label>
  );
}

interface RadioOptionProps {
  name: string;
  label: string;
  checked: boolean;
  onSelect: () => void;
}

export function RadioOption({ name, label, checked, onSelect }: RadioOptionProps) {
  return (
    <label className="flex h-8 cursor-pointer items-center gap-3 rounded-md px-2 text-sm text-fg hover:bg-hover">
      <input type="radio" name={name} className="size-4" checked={checked} onChange={onSelect} />
      {label}
    </label>
  );
}
