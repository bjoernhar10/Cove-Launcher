import { useMessages } from "../../i18n/index.ts";
import { CircleAlert, WifiOff, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { cx, unlessSet } from "../../lib/cx.ts";
import { Button } from "./Button.tsx";

export function Skeleton({ className }: { className?: string }) {
  return <div aria-hidden className={cx("animate-skeleton bg-skeleton", unlessSet(className, ["rounded-"], "rounded-md"), className)} />;
}

/** Screen-reader announcement for loading regions. */
export function LoadingRegion({ label, children, className }: { label: string; children: ReactNode; className?: string }) {
  return (
    <div role="status" aria-label={label} aria-busy className={className}>
      {children}
    </div>
  );
}

interface EmptyStateProps {
  icon: LucideIcon;
  title: string;
  description: string;
  actions?: ReactNode;
  compact?: boolean;
}

export function EmptyState({ icon: Icon, title, description, actions, compact = false }: EmptyStateProps) {
  return (
    <div className={cx("flex flex-col items-center text-center", compact ? "py-10" : "py-20")}>
      <div className="flex size-10 items-center justify-center rounded-lg border border-border bg-surface text-fg-muted">
        <Icon aria-hidden />
      </div>
      <h2 className="mt-4 text-base font-medium text-fg">{title}</h2>
      <p className="mt-1 max-w-sm text-sm text-fg-muted">{description}</p>
      {actions && <div className="mt-5 flex flex-wrap justify-center gap-2">{actions}</div>}
    </div>
  );
}

export function ErrorState({ message, onRetry }: { message: string; onRetry: () => void }) {
  const t = useMessages();
  return (
    <EmptyState
      icon={CircleAlert}
      title={t.common.errorTitle}
      description={message}
      actions={<Button onClick={onRetry}>{t.common.retry}</Button>}
      compact
    />
  );
}

interface ProgressBarProps {
  value: number;
  max: number;
  label: string;
  className?: string;
  tone?: "accent" | "muted";
}

/** Thin 4px progress bar. */
export function ProgressBar({ value, max, label, className, tone = "accent" }: ProgressBarProps) {
  const pct = max > 0 ? Math.min(100, Math.max(0, (value / max) * 100)) : 0;
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={value}
      className={cx("h-1 overflow-hidden rounded-full bg-active", className)}
    >
      <div
        className={cx("h-full rounded-full transition-[width] duration-200 ease-out", tone === "accent" ? "bg-accent" : "bg-fg-subtle")}
        style={{ width: `${String(pct)}%` }}
      />
    </div>
  );
}

export function Badge({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span className={cx("inline-flex h-5 items-center rounded-md border border-border px-2 text-xs text-fg-muted", className)}>
      {children}
    </span>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-flex h-5 min-w-5 items-center justify-center rounded-md border border-border bg-surface px-1 font-sans text-xs text-fg-subtle">
      {children}
    </kbd>
  );
}

export function Avatar({ name, className }: { name: string; className?: string }) {
  const initials = name
    .split(/\s+/)
    .map((part) => part[0] ?? "")
    .join("")
    .slice(0, 2)
    .toUpperCase();
  return (
    <span
      aria-hidden
      className={cx("inline-flex size-7 shrink-0 items-center justify-center rounded-full bg-active text-xs font-medium text-fg-muted", className)}
    >
      {initials}
    </span>
  );
}

/** Subtle hint that cached data is shown because the catalogue is unreachable. */
export function OfflineBadge() {
  const t = useMessages();
  return (
    <span
      role="status"
      title={t.common.offlineHint}
      className="inline-flex h-6 items-center gap-1 rounded-full border border-border px-2 text-xs text-fg-muted"
    >
      <WifiOff aria-hidden />
      Offline
    </span>
  );
}
