import type { ReactNode } from "react";
import { LoadingRegion, Skeleton } from "../../components/ui/Feedback.tsx";

export function Panel({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
  return (
    <section aria-label={title}>
      <h2 className="text-lg font-semibold tracking-tight text-fg">{title}</h2>
      {description && <p className="mt-1 text-sm text-fg-muted">{description}</p>}
      <div className="mt-6">{children}</div>
    </section>
  );
}

export function Group({ children }: { children: ReactNode }) {
  return <div className="divide-y divide-border border-y border-border">{children}</div>;
}

export function SettingsSkeleton() {
  return (
    <LoadingRegion label="Einstellungen werden geladen" className="flex flex-col gap-4">
      {[0, 1, 2].map((i) => (
        <Skeleton key={i} className="h-14" />
      ))}
    </LoadingRegion>
  );
}
