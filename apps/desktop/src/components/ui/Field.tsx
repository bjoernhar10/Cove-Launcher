import { ChevronDown } from "lucide-react";
import { useId, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes } from "react";
import { cx, unlessSet } from "../../lib/cx.ts";

const CONTROL =
  "h-9 rounded-md border border-border bg-surface px-3 text-sm text-fg placeholder:text-fg-subtle " +
  "transition-colors duration-150 ease-out hover:border-border-strong focus-visible:border-accent focus-visible:outline-offset-0 " +
  "disabled:opacity-50 aria-invalid:border-danger";

export function Input({ className, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  // Full width unless the caller sets a width.
  return <input className={cx(CONTROL, unlessSet(className, ["w-", "flex-"], "w-full"), className)} {...rest} />;
}

export interface SelectOption<T extends string> {
  value: T;
  label: string;
}

interface SelectProps<T extends string> extends Omit<SelectHTMLAttributes<HTMLSelectElement>, "onChange" | "value"> {
  value: T;
  options: readonly SelectOption<T>[];
  onChange: (value: T) => void;
}

export function Select<T extends string>({ value, options, onChange, className, ...rest }: SelectProps<T>) {
  return (
    <div className={cx("relative", className)}>
      <select
        className={cx(CONTROL, "w-full appearance-none pr-9")}
        value={value}
        onChange={(e) => {
          const next = options.find((o) => o.value === e.target.value);
          if (next) onChange(next.value);
        }}
        {...rest}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      <ChevronDown className="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-fg-subtle" aria-hidden />
    </div>
  );
}

interface FieldProps {
  label: string;
  description?: string;
  error?: string | null;
  children: (props: { id: string; "aria-describedby"?: string; "aria-invalid"?: boolean }) => ReactNode;
}

/** Label + control + help/error text with correct ARIA wiring. */
export function Field({ label, description, error, children }: FieldProps) {
  const id = useId();
  const helpId = `${id}-help`;
  const help = error ?? description;
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={id} className="text-sm font-medium text-fg">
        {label}
      </label>
      {children({ id, ...(help ? { "aria-describedby": helpId } : {}), ...(error ? { "aria-invalid": true } : {}) })}
      {help && (
        <p id={helpId} className={cx("text-xs", error ? "text-danger" : "text-fg-muted")}>
          {help}
        </p>
      )}
    </div>
  );
}

interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
  id?: string;
}

export function Switch({ checked, onChange, label, disabled = false, id }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      id={id}
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => {
        onChange(!checked);
      }}
      className={cx(
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors duration-150 ease-out disabled:opacity-50",
        checked ? "bg-accent" : "bg-border-strong",
      )}
    >
      <span
        aria-hidden
        className={cx(
          "inline-block size-4 rounded-full bg-on-accent transition-transform duration-150 ease-out",
          checked ? "translate-x-4.5" : "translate-x-0.5",
        )}
      />
    </button>
  );
}

/** A labelled settings row: text on the left, control on the right. */
export function SettingRow({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-8 py-4">
      <div className="min-w-0">
        <p className="text-sm font-medium text-fg">{title}</p>
        {description && <p className="mt-0.5 text-sm text-fg-muted">{description}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}
